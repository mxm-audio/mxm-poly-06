//! mxm-poly-06's editor.
//!
//! Built to `docs/briefs/mxm-poly-06.md`, which is the gating document — this module implements it
//! and does not re-decide it. In particular the brief owns:
//!
//! - **§10's section sequence**, Oscillator · Filter · Envelope · Amplifier · LFO · Voice ·
//!   Chorus — the signal flow, then what moves it, then how notes reach it, then the effect the
//!   machine shipped with. [`SECTIONS`] is written in that order so a reordering is a visible diff.
//! - **§5's disclosure**: the bender's range and the wheel's reach in the Amplifier card's footer, behind a
//!   labelled expander — `mxm-mono-01`'s idiom for its bend range.
//! - **§6's category/card inventory**, with derived musician pages and developer-only Parameters.
//! - **§7's identity accent** and **§8's displays**.
//!
//! # It is a panel, not a window
//!
//! [`panel`] takes a `Ui` and draws into it. It does not create a window, run an event loop, or own
//! a swapchain. That is what lets the same code be the plugin's CLAP editor and, later, a
//! standalone harness's contents.
//!
//! # Gestures
//!
//! Every edit is bracketed: `begin_set_parameter`, `set_parameter_normalized`, `end_set_parameter`,
//! in exactly one place — [`binding::Bound::apply`]. An unclosed gesture leaves a host's automation
//! lane latched, and it breaks the player's step editing outright.

pub mod binding;
pub mod sections;
mod visuals;

use std::collections::HashMap;
use std::sync::Arc;

use egui::Ui;
use mxm_ui::space::SPACE_5;
use mxm_ui::theme::Tokens;
use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;
use nice_plug_egui::{EguiEditorState, NiceEguiApp, create_egui_editor};

use crate::params::MxmPoly06Params;
use crate::telemetry::Telemetry;

/// The size the editor **opens** at — **derived, not chosen**: the quarter-4K budget hugged around
/// every page, which `tests::the_opening_size_is_the_budget_hugged` holds.
const REFERENCE: (u32, u32) = (1606, 670);

/// The six cards — brief §10's seven sections, the amplifier and the chorus after it as one —
/// **in sequence**.
pub const SECTIONS: &[Section] = &[
    Section::Voice,
    Section::Lfo,
    Section::Oscillator,
    Section::Filter,
    Section::Amplifier,
    Section::Envelope,
];

/// The runs a row break may not fall inside — design system §3.4.
///
/// The keyboard block with the LFO, both upstream of everything; the oscillator alone, since the
/// machine folds its mixing into it; the filter with the amplifier after it; the chorus that ends
/// the audio path, with the one envelope that drives filter and amplifier both.
#[cfg(test)]
const GROUPS: &[&[usize]] = &[&[0, 1], &[2], &[3, 4], &[5]];

/// The narrowest the window may be: the wider of one card with the panel's gutters and the app bar
/// at its last compact step. The bar decides it: `the_app_bar_holds_in_the_minimum_window` measures
/// it.
const MINIMUM: (u32, u32) = (446, 320);

/// The keyboard cursor's card for the app bar's Volume, outside the paging keys 0…5.
const VOLUME_CARD: u64 = 64;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Section {
    Oscillator,
    Filter,
    Envelope,
    /// The amplifier and the chorus it drives, as one card (R2's call, the owner's to overrule):
    /// hugged, the Chorus was one switch alone, and the Amplifier's caption already says its level
    /// drives the chorus.
    Amplifier,
    Lfo,
    Voice,
}

impl Section {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Oscillator => "Oscillator",
            Self::Filter => "Filter",
            Self::Envelope => "Envelope",
            Self::Amplifier => "Amplifier and chorus",
            Self::Lfo => "LFO",
            Self::Voice => "Voice",
        }
    }

    /// Which parameters this card draws, so a coverage test can ask rather than assume.
    #[cfg(test)]
    pub(crate) const fn parameters(self) -> &'static [&'static str] {
        match self {
            Self::Oscillator => &["range", "saw", "pulse", "pulsewidth", "sub", "noise"],
            Self::Filter => &["hpf", "cutoff", "resonance"],
            Self::Envelope => &["attack", "decay", "sustain", "release"],
            Self::Amplifier => &["level", "vcamode", "chorus"],
            Self::Lfo => &["lforate", "lfosync", "lfodelay"],
            Self::Voice => &["keyassign", "portamento", "bendrange", "lfomod"],
        }
    }
}

/// Which parameters the app bar draws: the master Volume, after the chorus, beside the level meter
/// (design system §3.1 slot 6). It belongs to no card.
#[cfg(test)]
pub(crate) const BAR_PARAMETERS: &[&str] = &["volume"];

/// Builds the editor. Called from `Plugin::editor`.
pub fn create(params: Arc<MxmPoly06Params>, telemetry: Arc<Telemetry>) -> Option<MxmPoly06Editor> {
    let state = EguiEditorState::from_size(
        nice_plug::editor::dpi::LogicalSize::new(REFERENCE.0, REFERENCE.1),
        1.0,
    );

    create_egui_editor(
        state,
        nice_plug_egui::RepaintNotifier::new(),
        nice_plug_egui::EguiNiceSettings {
            title: "mxm-poly-06".to_owned(),
            // **A fixed window**, for the mono editors' reasons: the layout never reflows, so a
            // window of any other size could only add empty space or clip. Scaling is the zoom
            // control in the app bar, which egui-baseview turns into a window resize.
            resize_hint: ResizeHint {
                size_constraints: nice_plug::editor::SizeConstraints::min_logical_size(
                    nice_plug::editor::dpi::LogicalSize::new(MINIMUM.0 as f32, MINIMUM.1 as f32),
                ),
                ..ResizeHint::RESIZABLE
            },
            ..Default::default()
        },
        MxmPoly06App::new(params, telemetry),
    )
}

/// The editor type the plugin exposes.
pub type MxmPoly06Editor = nice_plug_egui::EguiEditor<MxmPoly06App>;

/// The editor's own state: what the plugin does not own and the host does not need.
pub struct MxmPoly06App {
    params: Arc<MxmPoly06Params>,
    telemetry: Arc<Telemetry>,
    /// Set in `build`, because that is where nice-plug hands it over.
    gui_context: Option<GuiContext>,
    /// Musician pages (0), or the developer-only Parameters surface (127).
    view: usize,
    /// Open text-entry buffers, keyed by parameter id.
    text_entry: HashMap<&'static str, Option<String>>,
    /// The preset library and everything the browser needs across frames.
    presets: PresetUi,
    /// Where the keyboard is: a card, and a parameter inside it. Transient, like the text
    /// buffers — it is not a parameter and nothing durable reads it.
    nav: mxm_ui::navigation::State,
}

/// The app bar's preset controls and what they need between frames — `mxm-preset`'s, one for
/// every instrument.
pub use mxm_preset::PresetUi;

impl MxmPoly06App {
    pub fn new(params: Arc<MxmPoly06Params>, telemetry: Arc<Telemetry>) -> Self {
        let params_for_presets = Arc::clone(&params);
        Self {
            params,
            telemetry,
            gui_context: None,
            view: 0,
            text_entry: HashMap::new(),
            presets: PresetUi::new(params_for_presets.as_ref()),
            nav: mxm_ui::navigation::State::default(),
        }
    }
}

impl NiceEguiApp for MxmPoly06App {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        mxm_ui::theme::apply(&egui_ctx);
        mxm_ui::typography::apply(&egui_ctx);
        // Light by default, overridable with `MXM_EDITOR_THEME`. The reasoning, and why the
        // default is not `System`, lives on `mxm_ui::theme::preference`.
        egui_ctx.set_theme(mxm_ui::theme::preference());
        self.gui_context = Some(nice_gui_ctx);
        Ok(())
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_context) = self.gui_context.clone() else {
            return;
        };
        panel(
            ui,
            &self.params,
            &self.telemetry,
            &gui_context.param_setter(),
            &mut self.view,
            &mut self.text_entry,
            &mut self.presets,
            &mut self.nav,
        );
    }

    fn editor_closed(&mut self) {
        self.gui_context = None;
    }
}

/// The whole editor, as a panel.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    ui: &mut Ui,
    params: &MxmPoly06Params,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    view: &mut usize,
    text_entry: &mut HashMap<&'static str, Option<String>>,
    presets: &mut PresetUi,
    nav: &mut mxm_ui::navigation::State,
) {
    let tokens = &tokens_for(ui);

    // A frame every 50 ms while the editor is open: the level meter changes between input events,
    // and so does a developer-channel request, which a frame that waited for the pointer would
    // strand on a view where nothing animates.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));

    // The collection's developer channel (`plugins/AGENTS.md`): the view, and the Bender's
    // expander, whose state is the collapsing header's own memory that the Amplifier card's tree
    // reads.
    // One question, and both layers suspend on it: the paging renderer's `hold` and the cursor's
    // `inert` both ask whether another surface owns this frame's keyboard.
    let busy = presets.holds_the_keyboard() || text_entry.values().any(Option::is_some);
    mxm_ui::paging::editor::hold(ui.ctx(), busy);
    mxm_ui::paging::editor::developer_request(ui.ctx(), view, telemetry.take_view_request());

    // **The keyboard cursor moves before anything is drawn**, so a navigation arrow is consumed
    // here rather than also walking egui's own focus ring. It reads the registry and the exact
    // card rectangles the previous frame built, and it resolves the developer-view request first,
    // because which surface this frame is deciding who owns its keyboard.
    if *view == mxm_ui::paging::PARAMETERS {
        // This surface has no cards. Stop rather than merely hiding the outline, or its controls
        // lose their legacy bare-arrow editing to an invisible stale musician cursor — the app
        // bar's Volume card included.
        mxm_ui::navigation::stop(ui.ctx());
    } else {
        mxm_ui::navigation::paged_with_bar(ui.ctx(), nav, busy, &[VOLUME_CARD]);
    }
    if let Some(open) = telemetry.take_browser_request() {
        presets.set_browser_open(open);
    }
    // A theme, by index. Applied and not stored: this channel is how a screenshot run and a test
    // reach a state, and neither should overwrite the choice made in the control.
    if let Some(index) = telemetry.take_theme_request()
        && let Some(preference) = mxm_ui::theme::from_index(index)
    {
        ui.ctx().set_theme(preference);
    }
    // Nothing is disclosed since the Bender's controls moved onto the Voice card (2026-09-28): the
    // developer channel's CC 118 is consumed and changes nothing, as in mxm-mono-08.
    let _ = telemetry.take_disclosure_request();

    let peak = telemetry.take_peak();
    let clipped = telemetry.clipped();
    mxm_ui::AppBar::new("mxm-poly-06").show_with(
        ui,
        tokens,
        |ui| mxm_preset::ui::preset_row(ui, tokens, params, setter, presets),
        |ui| {
            if mxm_ui::shell::level_meter(ui, tokens, peak, clipped) {
                telemetry.clear_clip();
            }
            // Design system §3.1 slot 6: the master output sits beside its meter. It is after the
            // chorus, so it is not the patch's Level, which stays on the Amplifier card.
            mxm_ui::navigation::bar_card(ui, VOLUME_CARD, |ui| {
                ui.scope(|ui| {
                    sections::binding_for("volume", params)
                        .slider_inline(ui, tokens, setter, text_entry, 96.0);
                })
                .response
                .rect
            });
            mxm_ui::shell::zoom_control(ui);

            // §3.1 slot 5, and the same place the player keeps it: at the left end of the bar's
            // right-hand group. What the person picks is remembered for every MXM editor, so the
            // next one to open agrees with this one.
            mxm_ui::shell::editor_theme_control(ui);
        },
    );

    mxm_preset::ui::overlays(ui, tokens, params, setter, presets);

    // **The Parameters view has no tab.** It is the complete generated list, and an editor whose
    // own interface reaches every control does not need a second way to the same parameters in
    // front of a musician every day. It stays reachable: the developer channel still requests it
    // by index, which is what the CLI and a host's automation list use it for.
    // Navigation is derived by the paging renderer.

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(tokens.canvas)
                .inner_margin(egui::Margin::same(SPACE_5 as i8)),
        )
        .show(ui, |ui| {
            if *view == mxm_ui::paging::PARAMETERS {
                parameters_view(ui, tokens, params, setter, text_entry);
            } else {
                paged_view(ui, tokens, params, telemetry, setter, text_entry);
            }
        });
}

/// Every paging item, each floor computed from its card's tree in `ui`'s fonts every frame, and each
/// card exactly as wide as that floor: its ceiling is its floor (`plans/plan-editor-standard.md`
/// A1), with no usability minimum (A2).
pub fn page_items(ui: &Ui, params: &MxmPoly06Params) -> Vec<mxm_ui::paging::Item<'static>> {
    use mxm_ui::{
        flow::Card,
        paging::{Category as C, Item, Key},
    };
    SECTIONS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let floor = mxm_ui::tree::card_floor(ui, s.title(), &sections::card(ui, *s, params));
            Item {
                key: Key(i as u64),
                card: Card::new(s.title(), floor).capped(floor),
                category: match s {
                    Section::Voice => C::Performance,
                    Section::Lfo | Section::Envelope => C::Modulators,
                    Section::Oscillator => C::Generators,
                    Section::Filter | Section::Amplifier => C::Tone,
                },
                kind: s.title(),
            }
        })
        .collect()
}

fn paged_view(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmPoly06Params,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    entries: &mut HashMap<&'static str, Option<String>>,
) {
    ui.ctx().request_repaint();
    use mxm_ui::paging::Key;
    let items = page_items(ui, params);
    let text_editing = entries.values().any(Option::is_some);
    // The voice display's telemetry is read once, before anything is drawn.
    let mut live = sections::Live::new(params, telemetry, setter, entries);
    mxm_ui::paging::editor::show(
        ui,
        tokens,
        &items,
        &[&[Key(3), Key(4)]],
        text_editing,
        &mut |ui, index| sections::card(ui, SECTIONS[index], params),
        &mut |ui, _, leaf, rect| sections::paint(ui, tokens, leaf, rect, &mut live),
    );
}

/// The paging items as the editor computes them, from a context set up as an editor's is — three
/// passes in, so the weighted font cuts are bound — for tests, which have no editor `Ui` to hand.
#[cfg(test)]
pub(crate) fn test_items() -> Vec<mxm_ui::paging::Item<'static>> {
    let ctx = egui::Context::default();
    mxm_ui::typography::apply(&ctx);
    mxm_ui::theme::apply(&ctx);
    let params = MxmPoly06Params::default();
    let mut items = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            items = page_items(ui, &params);
        });
        output.textures_delta.clear();
    }
    items
}

/// The cards' floors in paging order, as [`test_items`] computes them.
#[cfg(test)]
pub(crate) fn test_floors() -> Vec<f32> {
    test_items().iter().map(|item| item.card.floor).collect()
}

/// §6's `Parameters` view: every parameter as a slider, which is the testing surface.
fn parameters_view(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmPoly06Params,
    setter: &ParamSetter<'_>,
    text_entry: &mut HashMap<&'static str, Option<String>>,
) {
    mxm_ui::shell::scroll_list(ui).show(ui, |ui| {
        let columns = if ui.available_width() >= 1000.0 { 3 } else { 2 };
        // Every parameter, routes included: this is the surface a host's automation list mirrors,
        // so a route a musician page draws nothing for while absent is still reachable here.
        let entries: Vec<_> = sections::ALL_IDS
            .iter()
            .map(|id| sections::binding_for(id, params))
            .chain(
                params
                    .routes
                    .parameters()
                    .into_iter()
                    .map(|(id, param)| binding::Bound {
                        id,
                        param,
                        description: "A modulation route's presence or depth.",
                        bipolar: !id.ends_with("on"),
                        law: binding::StepLaw::Own,
                        panel: None,
                        stepped: None,
                        details: &[],
                    }),
            )
            .collect();
        let per_column = entries.len().div_ceil(columns);

        ui.columns(columns, |uis| {
            for (index, chunk) in entries.chunks(per_column).enumerate() {
                let Some(column) = uis.get_mut(index) else {
                    continue;
                };
                for entry in chunk {
                    entry.slider(column, tokens, setter, text_entry);
                }
            }
        });
    });
}

/// The collection's tokens, with this instrument's identity accent (brief §7).
fn tokens_for(ui: &Ui) -> Tokens {
    let dark = ui.visuals().dark_mode;
    let base = if dark { mxm_ui::DARK } else { mxm_ui::LIGHT };
    base.with_identity(mxm_ui::theme::ROSE, dark)
}

#[cfg(test)]
mod tests {
    use mxm_plugin_test::{opening_size, paging_checks};

    /// **The editor opens at the quarter-4K budget, hugged** — the owner's rule, 2026-09-09. The budget
    /// is the most room an editor may ask for, so laying the panel out there shows as many modules as
    /// it ever will; taking the slack away is the whole of the size.
    #[test]
    fn the_opening_size_is_the_budget_hugged() {
        let params = MxmPoly06Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        opening_size::is_the_budget_hugged(
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &REVEAL,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    /// **The app bar holds in the narrowest window**: its `…` menu whole and nothing drawn over
    /// anything else, from `MINIMUM` up (`opening_size::bar_holds_from_the_minimum`).
    #[test]
    fn the_app_bar_holds_in_the_minimum_window() {
        let params = MxmPoly06Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        opening_size::bar_holds_from_the_minimum(
            egui::vec2(MINIMUM.0 as f32, MINIMUM.1 as f32),
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    use mxm_plugin_test::keyboard_checks;

    /// Nothing is kept behind a disclosure: the bend controls are on the Voice card.
    const REVEAL: fn(&egui::Context) = |_| {};

    /// Every route present, as the `‹ modulate ›` menu would add them one at a time. An absent
    /// route draws nothing at all, so a check at the init patch reaches six of eighty-eight.
    fn reveal_every_route(params: &MxmPoly06Params) {
        use nice_plug::params::InternalParamMut;
        for group in params.routes.each() {
            for presence in [
                &group.key_on,
                &group.env_on,
                &group.lfo_on,
                &group.vel_on,
                &group.wheel_on,
                &group.press_on,
                &group.bend_on,
                &group.saw_on,
                &group.pulse_on,
                &group.sub_on,
                &group.noise_on,
            ] {
                // SAFETY: a test owns these parameters outright; nothing else holds them.
                unsafe { presence._internal_set_plain_value(true) };
            }
        }
    }

    /// Every parameter the panel should draw: the cards' own, and a presence and an amount for
    /// every route that is present.
    fn drawn_ids(params: &MxmPoly06Params) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = sections::all_parameters(params)
            .iter()
            .map(|bound| bound.id)
            .collect();
        for (t, group) in params.routes.each().into_iter().enumerate() {
            for (s, present) in group.presences(t).into_iter().enumerate() {
                if present {
                    let (amount, presence) = crate::routes::ROUTE_IDS[t][s];
                    ids.push(amount);
                    ids.push(presence);
                }
            }
        }
        ids
    }

    /// The rollout's own failure mode: a control whose `navigation::at` scope was forgotten paints
    /// exactly as before and is simply unreachable from the keyboard. Nothing else would say so.
    /// **Two frames**: the init patch, and every route present.
    #[test]
    fn the_keyboard_cursor_reaches_and_operates_every_parameter() {
        reach_and_operate(false);
    }

    #[test]
    fn the_keyboard_cursor_reaches_and_operates_every_route_revealed() {
        reach_and_operate(true);
    }

    fn reach_and_operate(revealed: bool) {
        let params = MxmPoly06Params::default();
        if revealed {
            reveal_every_route(&params);
        }
        let telemetry = Telemetry::default();
        let host = keyboard_checks::Recorder::default();
        let setter = ParamSetter::new(&host);
        let ids = drawn_ids(&params);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        keyboard_checks::the_cursor_reaches_and_operates(
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &test_items(),
            keyboard_checks::Coverage::Exactly(&ids),
            &REVEAL,
            &host,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    /// **Volume is the app bar's, and no card draws it** (design system §3.1 slot 6). Whichever
    /// card is requested, it registers exactly once, under the bar card, whose key no page uses.
    #[test]
    fn the_master_volume_is_drawn_once_in_the_app_bar() {
        let items = test_items();
        assert!(
            items.iter().all(|item| item.key.0 != VOLUME_CARD),
            "the bar card's key collides with a paging key"
        );
        let params = MxmPoly06Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        let mut draw = |ui: &mut Ui| {
            panel(
                ui,
                &params,
                &telemetry,
                &setter,
                &mut view,
                &mut text_entry,
                &mut presets,
                &mut nav,
            );
        };
        let session =
            keyboard_checks::Session::new(egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32));
        for item in &items {
            mxm_ui::paging::editor::request_card(session.context(), item.key);
            REVEAL(session.context());
            session.settle(&mut draw);
            let cards: Vec<u64> = mxm_ui::navigation::spots(session.context())
                .into_iter()
                .filter(|spot| spot.key == "volume")
                .map(|spot| spot.card)
                .collect();
            assert_eq!(
                cards,
                [VOLUME_CARD],
                "with {} requested, Volume registers under {cards:?}",
                item.card.title
            );
        }
    }

    #[test]
    fn every_dynamic_page_fits_and_every_card_is_reachable() {
        let params = MxmPoly06Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0;
        let mut entries = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        paging_checks::verify(
            &test_items(),
            &[
                egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
                egui::vec2(1880.0, 1040.0),
                egui::vec2(MINIMUM.0 as f32, MINIMUM.1 as f32),
            ],
            |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut entries,
                    &mut presets,
                    &mut nav,
                )
            },
        );
    }
    use super::*;
    use nice_plug::params::internals::ParamPtr;
    use nice_plug::prelude::{PluginApi, PluginState};

    /// The editor reports edits through a `ParamSetter`; laying it out makes none.
    struct NoHost;

    impl nice_plug::context::gui::GuiContextInner for NoHost {
        // A test double has no host to ask for a restart (nice-plug 0.4).
        fn request_restart(&self) {}
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}
        unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: PluginState) {}
    }

    /// Lays the whole editor out at a given size and hands back the context and the height it
    /// actually needed.
    fn lay_out(view: usize, width: f32, height: f32) -> (egui::Context, f32) {
        let ctx = egui::Context::default();
        mxm_ui::theme::apply(&ctx);
        mxm_ui::typography::apply(&ctx);
        ctx.set_theme(egui::ThemePreference::Light);
        // The header animates open over several frames; a measurement wants the open state now.
        ctx.all_styles_mut(|style| style.animation_time = 0.0);

        let params = MxmPoly06Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = view;
        let mut text_entry = HashMap::new();
        // A library rooted nowhere: a layout test must never touch the real config directory.
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };

        let mut used = 0.0;
        for _ in 0..3 {
            let mut output = ctx.run_ui(input.clone(), |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
            output.textures_delta.clear();
            used = ctx.globally_used_rect().height();
        }
        (ctx, used)
    }

    /// What the editor drew at a given size.
    fn measure(view: usize, width: f32, height: f32) -> f32 {
        lay_out(view, width, height).1
    }

    /// The height the Synth view needs: where its levelled columns end, plus the panel's margin.
    /// Not `globally_used_rect`: the central panel fills the window whatever is in it, so that
    /// only exceeds the window once something has already been cut off — which is how a 760
    /// point window passed this test while the open Bender pushed the Chorus card off it.
    /// Where every card landed on the Synth view, at a given width.
    fn placed(width: f32) -> Vec<egui::Rect> {
        let (ctx, _) = lay_out(0, width, 20000.0);
        paging_checks::all_rects(&ctx, SECTIONS.len())
    }

    /// Cards whose vertical extents overlap are on one row, read off the geometry — what a person
    /// sees, and what catches a layout that is right in taffy and wrong on screen.
    fn rows(placed: &[egui::Rect]) -> Vec<Vec<egui::Rect>> {
        let mut sorted = placed.to_vec();
        sorted.sort_by(|a, b| {
            a.top()
                .partial_cmp(&b.top())
                .unwrap()
                .then(a.left().partial_cmp(&b.left()).unwrap())
        });
        let mut rows: Vec<Vec<egui::Rect>> = Vec::new();
        for rect in sorted {
            match rows.last_mut() {
                Some(row) if row.iter().any(|r| r.bottom() > rect.top() + 1.0) => row.push(rect),
                _ => rows.push(vec![rect]),
            }
        }
        rows
    }

    fn synth_height() -> f32 {
        placed(REFERENCE.0 as f32)
            .iter()
            .map(egui::Rect::bottom)
            .fold(0.0_f32, f32::max)
            + SPACE_5
    }

    /// **The size the editor opens at, measured rather than guessed.** The window resizes now, so
    /// this is not a frame the content must fit inside for ever — but opening scrolled is still
    /// wrong, so it is pinned from above and below.
    ///
    /// Nothing expands since the Bender's controls moved onto the Voice card (2026-09-28), so the
    /// view is measured as it is.
    #[test]
    fn the_synth_view_fits_the_editor() {
        eprintln!("the Synth view needs {} points", synth_height());
        // Fit is per derived page; the total surface no longer sizes the window.
        every_dynamic_page_fits_and_every_card_is_reachable();
    }

    /// **Every row of cards shares one bottom edge.**
    ///
    /// This replaced *the three columns end on one line*. There are no columns: the cards wrap into
    /// rows, and §3.3's rule is per row. A column test would have gone on passing while nothing
    /// lined up across the panel.
    #[test]
    fn every_row_of_cards_shares_one_bottom_edge() {
        for width in [MINIMUM.0 as f32, 700.0, 1000.0, 1200.0, 1600.0] {
            for row in rows(&placed(width)) {
                if row.len() < 2 {
                    continue;
                }
                let low = row
                    .iter()
                    .map(egui::Rect::bottom)
                    .fold(f32::INFINITY, f32::min);
                let high = row
                    .iter()
                    .map(egui::Rect::bottom)
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    high - low < 1.0,
                    "at {width}: a row of {} cards ends {:.1} ragged",
                    row.len(),
                    high - low
                );
            }
        }
    }

    /// Nothing is clipped, squeezed under its floor, grown past the cap, or overlapping.
    #[test]
    fn the_layout_reflows_without_clipping_or_overlap() {
        let floors = test_floors();
        for width in [MINIMUM.0 as f32, 700.0, 1000.0, 1200.0, 1600.0] {
            let placed = placed(width);
            for (index, rect) in placed.iter().enumerate() {
                assert!(
                    rect.left() >= -0.5 && rect.right() <= width + 0.5,
                    "{} runs from {:.1} to {:.1} at {width}",
                    SECTIONS[index].title(),
                    rect.left(),
                    rect.right()
                );
                assert!(
                    (rect.width() - floors[index]).abs() <= 0.5,
                    "{} is {:.1} wide, not its floor {:.1}",
                    SECTIONS[index].title(),
                    rect.width(),
                    floors[index]
                );
            }
            for (i, a) in placed.iter().enumerate() {
                for b in placed.iter().skip(i + 1) {
                    let overlap = a.intersect(*b);
                    assert!(
                        overlap.width() <= 0.5 || overlap.height() <= 0.5,
                        "two cards overlap at {width}"
                    );
                }
            }
        }
    }

    /// Every card, in every state that changes what it holds, passes the layout tree's checks
    /// (plans/plan-layout-tree.md §4.3, `tree_checks::card`): its floor holds its content with
    /// nothing painted outside the card, the content floor is exact, the height its tree states is
    /// the height it draws, and every leaf stays in the room it was given.
    ///
    /// The states are this editor's structural-state matrix: the init patch; every route revealed
    /// at full negative depth, where a reading carries its sign and every digit — the widest text a
    /// row can show — and the filter curve draws the envelope's reach; the Bender open; and every
    /// voice sounding the key with the longest name, the voice display's longest text. Every floor
    /// is the one the editor computes at Init, so a state that widened a card past it would fail
    /// the first check.
    #[test]
    fn every_card_passes_the_tree_checks_in_every_state() {
        use nice_plug::prelude::Params as _;
        let floors = test_floors();
        for state in [
            "init",
            "every route revealed",
            "every voice sounding",
            "LFO synced, no tempo",
            "LFO synced to a tempo",
        ] {
            let params = MxmPoly06Params::default();
            let telemetry = Telemetry::default();
            if state.starts_with("LFO synced") {
                // SAFETY: a test owns these parameters outright; nothing else holds them.
                unsafe {
                    use nice_plug::params::InternalParamMut;
                    let _ = params.lfo_sync._internal_set_normalized_value(1.0);
                }
            }
            if state == "LFO synced to a tempo" {
                telemetry.tempo.publish(Some(120.0));
            }
            if state == "every route revealed" {
                reveal_every_route(&params);
                let amounts: Vec<&str> = crate::routes::ROUTE_IDS
                    .iter()
                    .flat_map(|target| target.iter().map(|(amount, _)| *amount))
                    .collect();
                for (id, parameter, _) in params.param_map() {
                    if amounts.contains(&id.as_str()) {
                        // SAFETY: a test owns these parameters outright; nothing else holds them.
                        unsafe { parameter._internal_set_normalized_value(0.0) };
                    }
                }
            }
            if state == "every voice sounding" {
                let widest = (0..=127u8)
                    .max_by_key(|note| visuals::note_name(*note).len())
                    .unwrap_or(0);
                telemetry.publish_voices(
                    &[1.0; mxm_poly_06_dsp::voice::VOICES],
                    &[(widest, true); mxm_poly_06_dsp::voice::VOICES],
                );
            }
            let setup = |_: &egui::Context| {};
            let host = NoHost;
            let setter = ParamSetter::new(&host);
            for (index, floor) in floors.iter().enumerate() {
                let section = SECTIONS[index];
                let mut text_entry = HashMap::new();
                let mut live = sections::Live::new(&params, &telemetry, &setter, &mut text_entry);
                tree_checks::card(
                    &setup,
                    state,
                    section.title(),
                    *floor,
                    &|ui| sections::card(ui, section, &params),
                    &mut |ui, leaf, rect| {
                        sections::paint(ui, &mxm_ui::LIGHT, leaf, rect, &mut live);
                    },
                );
            }
        }
    }

    /// `sections::draw`, the layout lab's entry point, draws each card's own tree: at the card's
    /// floor, with the Bender open and every route revealed, its body is exactly as tall as the
    /// tree says and nothing is painted outside the card. The lab's own command cannot build this
    /// instrument alone, so this is where the entry point is held.
    #[test]
    fn the_lab_entry_point_draws_every_cards_tree() {
        let floors = test_floors();
        let params = MxmPoly06Params::default();
        reveal_every_route(&params);
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        for (index, section) in SECTIONS.iter().enumerate() {
            let ctx = tree_checks::context(&|_| {});
            let mut text_entry = HashMap::new();
            let (mut card, mut body, mut stated, mut painted) = (
                egui::Rect::NOTHING,
                egui::Rect::NOTHING,
                0.0,
                egui::Rect::NOTHING,
            );
            for _ in 0..3 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let mut column =
                        ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(floors[index], 4000.0),
                        )));
                    let shown = mxm_ui::ModuleCard::new(section.title()).show(
                        &mut column,
                        &mxm_ui::LIGHT,
                        |ui| {
                            let tree = sections::card(ui, *section, &params);
                            stated = tree.height(ui, tree.drawn_width(ui, ui.available_width()));
                            let top = ui.cursor().top();
                            sections::draw(
                                ui,
                                &mxm_ui::LIGHT,
                                *section,
                                &params,
                                &telemetry,
                                &setter,
                                &mut text_entry,
                                0.0,
                            );
                            egui::Rect::from_min_max(
                                egui::pos2(ui.min_rect().left(), top),
                                ui.min_rect().max,
                            )
                        },
                    );
                    body = shown;
                    card = column.min_rect();
                });
                painted = output
                    .shapes
                    .iter()
                    .map(|clipped| clipped.shape.visual_bounding_rect())
                    .filter(|rect| rect.is_finite() && rect.is_positive())
                    .fold(egui::Rect::NOTHING, |a, b| a.union(b));
                output.textures_delta.clear();
            }
            assert!(
                (body.height() - stated).abs() < 0.5,
                "{}: the tree said {stated:.1}, the lab's entry point drew {:.1}",
                section.title(),
                body.height()
            );
            assert!(
                card.expand(0.5).contains_rect(painted),
                "{}: painted {painted:?} outside its card {card:?}",
                section.title()
            );
        }
    }

    /// `Parameters` is allowed to be taller: it is the one view that scrolls.
    #[test]
    fn the_parameters_view_lays_out_without_panicking() {
        assert!(
            measure(
                mxm_ui::paging::PARAMETERS,
                REFERENCE.0 as f32,
                REFERENCE.1 as f32
            ) > 0.0
        );
    }

    /// The section order is the contract, so it is asserted rather than assumed.
    #[test]
    fn the_sections_are_in_the_briefs_order() {
        let titles: Vec<&str> = SECTIONS.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            [
                "Voice",
                "LFO",
                "Oscillator",
                "Filter",
                "Amplifier and chorus",
                "Envelope",
            ]
        );
    }

    /// Every card is in exactly one group, and no group names a card that does not exist.
    ///
    /// The groups are what a row break may not fall inside (§3.4). A card left out of them would
    /// still be drawn — `flow::pack` takes the groups as given — but it would wrap on its own, and
    /// the pairing the brief argued for would quietly stop holding.
    #[test]
    fn every_card_is_in_exactly_one_group() {
        let mut seen = vec![0usize; SECTIONS.len()];
        for group in GROUPS {
            for card in *group {
                assert!(
                    *card < SECTIONS.len(),
                    "group names card {card}, which does not exist"
                );
                seen[*card] += 1;
            }
        }
        for (card, count) in seen.iter().enumerate() {
            assert_eq!(
                *count,
                1,
                "{} is in {count} groups; it must be in exactly one",
                SECTIONS[card].title()
            );
        }
    }

    use mxm_plugin_test::tree_checks;

    /// Every page at the opening size, light and dark, for the owner's review of the layout-tree
    /// conversion (plans/plan-layout-tree.md §4.3): `target/layout-tree/mxm-poly-06/<tag>/`, where
    /// `MXM_PICTURES` names the tag.
    ///
    /// `MXM_PICTURES=after cargo test -p mxm-poly-06 --lib tree_pictures -- --ignored`
    #[test]
    #[ignore = "renders through wgpu; run by hand"]
    fn tree_pictures() {
        let tag = std::env::var("MXM_PICTURES").unwrap_or_else(|_| "after".to_owned());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/layout-tree/mxm-poly-06")
            .join(tag);
        let params = MxmPoly06Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        tree_checks::pictures(
            &|_| {},
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &dir,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    /// Card names come from the collection's shared vocabulary; only **Chorus** is new, and it is
    /// new because the thing is: this is the collection's first built-in effect.
    #[test]
    fn card_names_come_from_the_collections_vocabulary() {
        const KNOWN: &[&str] = &[
            "LFO",
            "Oscillator",
            "Mixer",
            "Filter",
            "Amplifier and chorus",
            "Envelope",
            "Voice",
        ];
        for title in SECTIONS.iter().map(|s| s.title()) {
            assert!(
                KNOWN.contains(&title),
                "{title} is a card name the collection does not use"
            );
        }
    }
}
