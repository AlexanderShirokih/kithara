//! What the two hosts owe each other about a modal: the scrim it lays over the
//! whole window, the surface it centres on it, and the input it keeps.
//!
//! The page under the modal holds a pressable and a knob, so a press the scrim
//! failed to take would show up as a write the page published.

use iced::{Background, Color, Rectangle, Vector, advanced::renderer::Quad};
use kithara_test_utils::kithara;
use kithara_ui::{
    app::{App, Config, Ui},
    backends::paint_color,
    builtin,
    compile::{CompiledUi, compile},
    draw::{Pt, Rect, Rgba},
    ids::EndpointId,
    interact::{Input, InputMethod, Key, MOUSE, Modifiers, PointerInput, PointerPhase, Scroll},
    registry::{EndpointCategory, EndpointDesc, EndpointRegistry, ValueKind},
    render::{ReadValue, Reads, Scope, Skin, UiEvent, WriteValue},
    skin::ColorRole,
    source::{MemResolver, UiConfig},
    view,
};

use crate::immediate::Immediate;

/// The window both hosts open the page in.
const WINDOW: (u32, u32) = (480, 320);

/// The page: a knob and a search field in a strip along the top and a
/// pressable filling the rest. `{modal}` stands first, so a modal that took
/// room would push them all down.
const PAGE: &str = r#"Column(size: (w: Fill, h: Fill), gap: 0.0, pad: 0.0, children: [
    {modal}
    Row(size: (w: Fill, h: Fixed(60.0)), gap: 0.0, pad: 0.0, children: [
        Knob(id: "dial", size: (w: Fixed(38.0), h: Fixed(49.0)),
            read: Model(id: "fixture.dial"), write: Parameter(id: "fixture.dial")),
        Search(id: "query", size: (w: Fixed(160.0), h: Fixed(26.0)),
            read: Model(id: "fixture.query"), write: Command(id: "fixture.query")),
    ]),
    Pressable(id: "page", press: Command(id: "fixture.page"),
        child: Spacer(id: "page-face", size: Some((w: Fill, h: Fill)))),
])"#;

/// A modal over the page, its content a quiet strip above a pressable row.
fn modal(width: f32, height: f32) -> String {
    format!(
        r#"Modal(id: "settings", open: Model(id: "fixture.open"),
            close: Command(id: "fixture.close"),
            content: Column(id: "surface", size: (w: Fixed({width:.1}), h: Fixed({height:.1})),
                gap: 0.0, pad: 0.0, children: [
                    Spacer(id: "inside", size: Some((w: Fill, h: Fixed(40.0)))),
                    Pressable(id: "pick", press: Command(id: "fixture.pick"),
                        child: Spacer(id: "pick-face", size: Some((w: Fill, h: Fixed(20.0))))),
                ])),"#
    )
}

/// What the page holds: no modal, or a modal whose content asks for a size.
#[derive(Clone, Copy)]
enum Holds {
    Nothing,
    Modal(f32, f32),
}

impl Holds {
    const SMALL: Self = Self::Modal(100.0, 60.0);
    const OVERSIZE: Self = Self::Modal(600.0, 400.0);

    fn documents(self) -> MemResolver {
        let modal = match self {
            Self::Nothing => String::new(),
            Self::Modal(width, height) => modal(width, height),
        };
        let mut resolver = MemResolver::default();
        resolver.insert(
            "page.klayout.ron",
            r#"(schema: "kithara.layout", version: 1, id: "page",
                root: Module(instance: "demo", source: "page.kmodule.ron", size: (w: Fill, h: Fill)))"#,
        );
        resolver.insert(
            "page.kmodule.ron",
            &format!(
                r#"(schema: "kithara.module", version: 1, id: "page", chrome: Plain,
                    root: {})"#,
                PAGE.replace("{modal}", &modal)
            ),
        );
        resolver
    }
}

/// The geometry a 100x60 content column takes in the window: one frame pixel
/// around it, the whole surface centred.
mod small {
    use super::Rect;

    pub(super) const SURFACE: Rect = Rect {
        x: 189.0,
        y: 129.0,
        w: 102.0,
        h: 62.0,
    };
    pub(super) const CONTENT: Rect = Rect {
        x: 190.0,
        y: 130.0,
        w: 100.0,
        h: 60.0,
    };
}

/// The geometry content larger than the window shrinks to: the window less
/// the reach of the shadow on every side, so the whole shadow stays in it.
mod oversize {
    use super::Rect;

    pub(super) const SURFACE: Rect = Rect {
        x: 80.0,
        y: 104.0,
        w: 320.0,
        h: 112.0,
    };
    pub(super) const CONTENT: Rect = Rect {
        x: 81.0,
        y: 105.0,
        w: 318.0,
        h: 110.0,
    };
}

/// The modal's look in the dark skin, as the settings window of the design
/// canon draws it.
mod look {
    pub(super) const SCRIM_ALPHA: f32 = 0.72;
    pub(super) const SHADOW_ALPHA: f32 = 0.55;
    pub(super) const SHADOW_BLUR: f32 = 80.0;
    pub(super) const SHADOW_OFFSET_Y: f32 = 24.0;
    pub(super) const TICK_SIZE: f32 = 10.0;
    pub(super) const TICK_WIDTH: f32 = 2.0;
}

fn role(role: ColorRole) -> Rgba {
    builtin::skin().palette[role]
}

fn faded(role_: ColorRole, alpha: f32) -> Rgba {
    Rgba {
        a: alpha,
        ..role(role_)
    }
}

/// The application: it answers the modal's flag and keeps every event the
/// document publishes, so what a gesture reached is what it published.
#[derive(Default)]
struct Page {
    open: bool,
    published: Vec<UiEvent>,
    query: String,
}

impl Page {
    fn open() -> Self {
        Self {
            open: true,
            ..Self::default()
        }
    }
}

impl Reads for Page {
    fn get(&self, endpoint: &str) -> Option<ReadValue<'_>> {
        match Scope::split(endpoint).0 {
            "fixture.open" => Some(ReadValue::Bool(self.open)),
            "fixture.dial" => Some(ReadValue::Scalar(0.5)),
            "fixture.query" => Some(ReadValue::Text(&self.query)),
            _ => None,
        }
    }
}

impl App for Page {
    fn document(&self) -> &str {
        "page.klayout.ron"
    }

    fn reads<R>(&self, with: impl FnOnce(&dyn Reads) -> R) -> R {
        with(self)
    }

    fn skin(&self) -> &Skin {
        builtin::skin()
    }

    /// The first query typed opens the modal, so the field it was typed into
    /// still holds the keyboard when the modal stands.
    fn update(&mut self, event: UiEvent) {
        if let UiEvent::Write {
            key,
            value: WriteValue::Text(query),
        } = &event
            && key == "fixture.query"
        {
            self.query.clone_from(query);
            self.open = true;
        }
        self.published.push(event);
    }
}

struct Endpoints {
    flag: EndpointDesc,
    scalar: EndpointDesc,
    text: EndpointDesc,
    trigger: EndpointDesc,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            flag: EndpointDesc::new(ValueKind::Bool),
            scalar: EndpointDesc::new(ValueKind::Scalar),
            text: EndpointDesc::new(ValueKind::Text),
            trigger: EndpointDesc::new(ValueKind::Trigger),
        }
    }
}

impl EndpointRegistry for Endpoints {
    fn endpoint(&self, category: EndpointCategory, id: &EndpointId) -> Option<&EndpointDesc> {
        match (category, id.0.as_str()) {
            (EndpointCategory::Model, "fixture.open") => Some(&self.flag),
            (EndpointCategory::Model, "fixture.dial")
            | (EndpointCategory::Parameter, "fixture.dial") => Some(&self.scalar),
            (EndpointCategory::Model | EndpointCategory::Command, "fixture.query") => {
                Some(&self.text)
            }
            (EndpointCategory::Command, "fixture.close" | "fixture.page" | "fixture.pick") => {
                Some(&self.trigger)
            }
            _ => None,
        }
    }
}

fn trigger(key: &str) -> UiEvent {
    UiEvent::Write {
        key: key.to_owned(),
        value: WriteValue::Trigger,
    }
}

fn centre(rect: Rect) -> Pt {
    Pt {
        x: rect.x + rect.w / 2.0,
        y: rect.y + rect.h / 2.0,
    }
}

/// One step of a gesture, played the same way on both hosts.
#[derive(Clone, Copy)]
enum Step {
    Click(Pt),
    /// Presses at the first point, travels to the second and lets go there.
    Drag(Pt, Pt),
    Escape,
    /// Types one character, the key that types it named for the immediate host.
    Type(&'static str, iced::keyboard::key::Code),
    /// An input method commits text, as a paste reaches a field.
    Commit(&'static str),
    /// Notches of the wheel over a point, the pointer arriving there first.
    Wheel(Pt, f32),
}

/// Mounts the page on the retained host and hands it to the check.
fn with_retained<R>(holds: Holds, app: Page, check: impl FnOnce(&mut Ui<'_, Page>) -> R) -> R {
    let endpoints = Endpoints::default();
    let resolver = holds.documents();
    let mut ui = Ui::new(
        app,
        Config::builder()
            .endpoints(&endpoints)
            .resolver(&resolver)
            .text(builtin::text_doc())
            .build(),
        WINDOW,
        1.0,
    )
    .unwrap_or_else(|error| panic!("the page must mount on the retained host: {error}"));
    check(&mut ui)
}

fn pointer(ui: &mut Ui<'_, Page>, phase: PointerPhase, at: Pt) {
    ui.input(Input::Pointer(PointerInput::new(
        MOUSE,
        None,
        phase,
        Some(at),
        1,
    )));
}

fn play_retained(ui: &mut Ui<'_, Page>, steps: &[Step]) {
    for step in steps {
        match *step {
            Step::Click(at) => {
                for phase in [PointerPhase::Move, PointerPhase::Down, PointerPhase::Up] {
                    pointer(ui, phase, at);
                }
            }
            Step::Drag(from, to) => {
                pointer(ui, PointerPhase::Move, from);
                pointer(ui, PointerPhase::Down, from);
                pointer(ui, PointerPhase::Move, to);
                pointer(ui, PointerPhase::Up, to);
            }
            Step::Escape => {
                ui.input(Input::KeyPressed {
                    key: Key::Escape,
                    modifiers: Modifiers::default(),
                    text: None,
                });
                ui.input(Input::KeyReleased {
                    key: Key::Escape,
                    modifiers: Modifiers::default(),
                });
            }
            Step::Type(text, _) => {
                ui.input(Input::KeyPressed {
                    key: Key::character(text, None),
                    modifiers: Modifiers::default(),
                    text: Some(text),
                });
                ui.input(Input::KeyReleased {
                    key: Key::character(text, None),
                    modifiers: Modifiers::default(),
                });
            }
            Step::Commit(text) => {
                ui.input(Input::InputMethod(InputMethod::Commit(text)));
            }
            Step::Wheel(at, notches) => {
                pointer(ui, PointerPhase::Move, at);
                ui.input(Input::Wheel(Scroll::Lines { x: 0.0, y: notches }));
            }
        }
    }
}

fn compiled(holds: Holds) -> CompiledUi {
    compile(
        "page.klayout.ron",
        &holds.documents(),
        &Endpoints::default(),
        builtin::skin_doc(),
        builtin::text_doc(),
        &UiConfig::default(),
        &view::EMPTY,
    )
    .unwrap_or_else(|error| panic!("the page must compile: {error}"))
}

fn play_immediate(holds: Holds, app: Page, steps: &[Step]) -> Vec<UiEvent> {
    let ui = compiled(holds);
    let mut host = Immediate::mount(app, &ui, builtin::skin(), WINDOW);
    for step in steps {
        match *step {
            Step::Click(at) => {
                host.click_at(at);
            }
            Step::Drag(from, to) => {
                host.press_at(from);
                host.hover_at(to);
                host.release_at(to);
            }
            Step::Escape => {
                host.key_at(
                    Pt { x: 1.0, y: 1.0 },
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    iced::keyboard::key::Code::Escape,
                );
            }
            Step::Type(text, code) => {
                host.key_at(
                    Pt { x: 1.0, y: 1.0 },
                    iced::keyboard::Key::Character(text.into()),
                    code,
                );
            }
            Step::Commit(text) => {
                host.commit_at(Pt { x: 1.0, y: 1.0 }, text);
            }
            Step::Wheel(at, notches) => {
                host.wheel_at(at, notches);
            }
        }
    }
    host.app().published.clone()
}

/// What each host published for the gesture, retained first.
fn both(holds: Holds, open: bool, steps: &[Step]) -> [Vec<UiEvent>; 2] {
    let app = || Page {
        open,
        ..Page::default()
    };
    let retained = with_retained(holds, app(), |ui| {
        play_retained(ui, steps);
        ui.app().published.clone()
    });
    [retained, play_immediate(holds, app(), steps)]
}

/// Where the page's controls stand on the retained host, with nothing over
/// them.
fn page_points() -> (Pt, Pt) {
    with_retained(Holds::Nothing, Page::default(), |ui| {
        let face = ui
            .rect_of("demo/page-face")
            .unwrap_or_else(|| panic!("the page pressable must be laid out"));
        let dial = ui
            .rect_of("demo/dial")
            .unwrap_or_else(|| panic!("the knob must be laid out"));
        let face = Pt {
            x: face.x + face.w - 20.0,
            y: face.y + face.h - 20.0,
        };
        let dial = centre(dial);
        (face, dial)
    })
}

/// The quads the immediate host draws for the page.
fn immediate_quads(holds: Holds, open: bool) -> Vec<(Rectangle, Quad, Background)> {
    let ui = compiled(holds);
    let mut host = Immediate::mount(
        Page {
            open,
            ..Page::default()
        },
        &ui,
        builtin::skin(),
        WINDOW,
    );
    host.quads()
}

fn iced_rect(rect: Rect) -> Rectangle {
    Rectangle {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
    }
}

fn iced_color(color: Rgba) -> Color {
    Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

fn filled(quads: &[(Rectangle, Quad, Background)], bounds: Rectangle, color: Rgba) -> bool {
    quads.iter().any(|(_, quad, background)| {
        quad.bounds == bounds && *background == Background::Color(iced_color(color))
    })
}

/// The four bars of the two corner ticks a surface carries, top-left and
/// bottom-right, each lying over the frame.
fn ticks(surface: Rect) -> [Rect; 4] {
    let (size, width) = (look::TICK_SIZE, look::TICK_WIDTH);
    let right = surface.x + surface.w;
    let bottom = surface.y + surface.h;
    [
        Rect {
            x: surface.x,
            y: surface.y,
            w: size,
            h: width,
        },
        Rect {
            x: surface.x,
            y: surface.y,
            w: width,
            h: size,
        },
        Rect {
            x: right - size,
            y: bottom - width,
            w: size,
            h: width,
        },
        Rect {
            x: right - width,
            y: bottom - size,
            w: width,
            h: size,
        },
    ]
}

/// Where the shadow of a surface inks: offset down and spread by its blur.
fn shadow_ink(surface: Rect) -> Rectangle {
    Rectangle {
        x: surface.x - look::SHADOW_BLUR,
        y: surface.y + look::SHADOW_OFFSET_Y - look::SHADOW_BLUR,
        width: surface.w + look::SHADOW_BLUR * 2.0,
        height: surface.h + look::SHADOW_BLUR * 2.0,
    }
}

fn covers(outer: Rectangle, inner: Rectangle) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

/// The surface quad the immediate host drew, carrying frame and shadow.
fn surface_quad(quads: &[(Rectangle, Quad, Background)], surface: Rect) -> (Rectangle, Quad) {
    quads
        .iter()
        .find(|(_, quad, background)| {
            quad.bounds == iced_rect(surface)
                && *background == Background::Color(iced_color(role(ColorRole::BgPanel)))
        })
        .map(|(layer, quad, _)| (*layer, *quad))
        .unwrap_or_else(|| panic!("no surface quad at {surface:?} in {quads:#?}"))
}

fn assert_immediate_draws(holds: Holds, surface: Rect) {
    let quads = immediate_quads(holds, true);
    let window = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 480.0,
        height: 320.0,
    };

    assert!(
        filled(&quads, window, faded(ColorRole::BgDeep, look::SCRIM_ALPHA)),
        "the scrim must cover the whole window: {quads:#?}"
    );
    let (layer, quad) = surface_quad(&quads, surface);
    assert_eq!(quad.border.width, 1.0);
    assert_eq!(quad.border.color, iced_color(role(ColorRole::Line)));
    assert_eq!(
        quad.shadow.color,
        iced_color(faded(ColorRole::Shadow, look::SHADOW_ALPHA))
    );
    assert_eq!(quad.shadow.offset, Vector::new(0.0, look::SHADOW_OFFSET_Y));
    assert_eq!(quad.shadow.blur_radius, look::SHADOW_BLUR);
    let ink = shadow_ink(surface);
    assert!(
        covers(layer, ink),
        "the shadow inks {ink:?}, outside the layer {layer:?} it was drawn in"
    );
    assert!(
        covers(window, ink),
        "the shadow inks {ink:?}, outside the window"
    );
    for tick in ticks(surface) {
        assert!(
            filled(&quads, iced_rect(tick), role(ColorRole::Accent)),
            "no corner tick at {tick:?}"
        );
    }
}

fn packed(color: Rgba) -> u32 {
    paint_color(color).premultiply().to_rgba8().to_u32()
}

/// How many times the retained host's picture names one colour.
fn painted(ui: &mut Ui<'_, Page>, color: Rgba) -> usize {
    let scene = ui
        .scene()
        .unwrap_or_else(|error| panic!("the retained host must draw: {error}"));
    let word = packed(color);
    scene
        .encoding()
        .draw_data
        .iter()
        .filter(|drawn| **drawn == word)
        .count()
}

fn assert_retained_draws(holds: Holds, surface: Rect, content: Rect) {
    let colors = [
        ("scrim", faded(ColorRole::BgDeep, look::SCRIM_ALPHA)),
        ("background", role(ColorRole::BgPanel)),
        ("frame", role(ColorRole::Line)),
        ("ticks", role(ColorRole::Accent)),
    ];
    let hidden: Vec<usize> = with_retained(holds, Page::default(), |ui| {
        colors
            .iter()
            .map(|(_, color)| painted(ui, *color))
            .collect()
    });
    with_retained(holds, Page::open(), |ui| {
        assert_eq!(
            ui.rect_of("demo/inside"),
            Some(Rect { h: 40.0, ..content }),
            "the content's first row must stand at the top of the content, centred inside its \
             frame"
        );
        for ((name, color), before) in colors.iter().zip(&hidden) {
            assert!(
                painted(ui, *color) > *before,
                "the shown modal must paint its {name}"
            );
        }
        let scene = ui
            .scene()
            .unwrap_or_else(|error| panic!("the retained host must draw: {error}"));
        let encoding = scene.encoding();
        let blur = [
            packed(faded(ColorRole::Shadow, look::SHADOW_ALPHA)),
            surface.w.to_bits(),
            surface.h.to_bits(),
            0.0_f32.to_bits(),
            (look::SHADOW_BLUR / 2.0).to_bits(),
        ];
        assert!(
            encoding
                .draw_data
                .windows(blur.len())
                .any(|drawn| drawn == blur),
            "the retained host must blur a {}x{} shadow out of the surface",
            surface.w,
            surface.h
        );
        let at = [
            surface.x + surface.w / 2.0,
            surface.y + surface.h / 2.0 + look::SHADOW_OFFSET_Y,
        ];
        assert!(
            encoding
                .transforms
                .iter()
                .any(|transform| transform.translation == at),
            "the shadow must centre at {at:?}, under the surface by its offset"
        );
    });
}

/// A shown modal centres its surface on the window and draws the scrim, the
/// frame, the corner ticks and the shadow, at the same geometry on both hosts.
#[kithara::test]
fn a_shown_modal_draws_its_scrim_surface_ticks_and_shadow_on_the_immediate_host() {
    assert_immediate_draws(Holds::SMALL, small::SURFACE);
}

#[kithara::test]
fn a_shown_modal_draws_its_scrim_surface_ticks_and_shadow_on_the_retained_host() {
    assert_retained_draws(Holds::SMALL, small::SURFACE, small::CONTENT);
}

/// Content larger than the window shrinks to the window less the shadow's
/// reach, so the surface stays centred and the shadow is drawn whole.
#[kithara::test]
fn oversize_content_shrinks_to_the_window_with_its_shadow_whole() {
    assert_immediate_draws(Holds::OVERSIZE, oversize::SURFACE);
    assert_retained_draws(Holds::OVERSIZE, oversize::SURFACE, oversize::CONTENT);
}

/// A press on the scrim writes the modal's close binding, and neither the
/// pressable nor the knob it covers hears it.
#[kithara::test]
fn a_press_on_the_scrim_closes_the_modal_and_reaches_nothing_under_it() {
    let (face, dial) = page_points();
    let up = Pt {
        x: dial.x,
        y: dial.y - 20.0,
    };
    let steps = [Step::Click(face), Step::Drag(dial, up)];

    let [bare, _] = both(Holds::Nothing, false, &steps);
    assert!(
        bare.contains(&trigger("fixture.page"))
            && bare
                .iter()
                .any(|event| matches!(event, UiEvent::Write { key, .. } if key == "fixture.dial")),
        "with nothing over them the press and the drag must reach the page: {bare:?}"
    );

    let [retained, immediate] = both(Holds::SMALL, true, &steps);
    let closed = vec![trigger("fixture.close"), trigger("fixture.close")];
    assert_eq!(retained, closed, "the retained host");
    assert_eq!(immediate, closed, "the immediate host");
}

/// Escape writes the close binding wherever the pointer rests.
#[kithara::test]
fn escape_closes_the_modal() {
    let [retained, immediate] = both(Holds::SMALL, true, &[Step::Escape]);

    assert_eq!(retained, [trigger("fixture.close")], "the retained host");
    assert_eq!(immediate, [trigger("fixture.close")], "the immediate host");
}

/// A press inside the content belongs to the content: on a quiet part of it
/// nothing is written, and on its pressable row that row's binding is.
#[kithara::test]
fn a_press_inside_the_modal_reaches_its_content_and_never_closes_it() {
    let (inside, pick) = with_retained(Holds::SMALL, Page::open(), |ui| {
        let inside = ui
            .rect_of("demo/inside")
            .unwrap_or_else(|| panic!("the modal content must be laid out"));
        let pick = ui
            .rect_of("demo/pick-face")
            .unwrap_or_else(|| panic!("the modal row must be laid out"));
        (centre(inside), centre(pick))
    });

    let [retained, immediate] = both(
        Holds::SMALL,
        true,
        &[Step::Click(inside), Step::Click(pick)],
    );

    assert_eq!(retained, [trigger("fixture.pick")], "the retained host");
    assert_eq!(immediate, [trigger("fixture.pick")], "the immediate host");
}

/// A modal its flag holds shut draws nothing, takes no room and no press: the
/// page is the page it would be with no modal in it.
#[kithara::test]
fn a_hidden_modal_leaves_the_page_as_if_it_were_not_there() {
    let (face, _) = page_points();
    let bare = with_retained(Holds::Nothing, Page::default(), |ui| {
        let scene = ui
            .scene()
            .unwrap_or_else(|error| panic!("the bare page must draw: {error}"));
        (ui.rect_of("demo/dial"), scene.encoding().draw_data.clone())
    });
    let shut = with_retained(Holds::SMALL, Page::default(), |ui| {
        let scene = ui
            .scene()
            .unwrap_or_else(|error| panic!("the page must draw: {error}"));
        (ui.rect_of("demo/dial"), scene.encoding().draw_data.clone())
    });
    assert_eq!(shut.0, bare.0, "a shut modal must take no room in the flow");
    assert_eq!(
        shut.1, bare.1,
        "a shut modal must draw nothing on the retained host"
    );

    assert_eq!(
        immediate_quads(Holds::SMALL, false),
        immediate_quads(Holds::Nothing, false),
        "a shut modal must draw nothing on the immediate host"
    );

    let [retained, immediate] = both(Holds::SMALL, false, &[Step::Click(face)]);
    assert_eq!(retained, [trigger("fixture.page")], "the retained host");
    assert_eq!(immediate, [trigger("fixture.page")], "the immediate host");
}

/// A key belongs to the modal even while a field under it holds the keyboard:
/// the field typed into before the modal opened hears nothing more, and Escape
/// closes the modal.
#[kithara::test]
fn a_standing_modal_keeps_the_keyboard_from_the_field_under_it() {
    use iced::keyboard::key::Code;

    let field = with_retained(Holds::SMALL, Page::default(), |ui| {
        ui.rect_of("demo/query")
            .unwrap_or_else(|| panic!("the search field must be laid out"))
    });
    let steps = [
        Step::Click(centre(field)),
        Step::Type("a", Code::KeyA),
        Step::Type("b", Code::KeyB),
        Step::Escape,
    ];

    let typed = UiEvent::Write {
        key: "fixture.query".to_owned(),
        value: WriteValue::Text("a".to_owned()),
    };
    let [retained, immediate] = both(Holds::SMALL, false, &steps);
    assert_eq!(
        retained,
        [typed.clone(), trigger("fixture.close")],
        "the retained host"
    );
    assert_eq!(
        immediate,
        [typed, trigger("fixture.close")],
        "the immediate host"
    );
}

/// Text an input method commits, which is how a paste reaches a field, belongs
/// to the modal too: the field focused before the modal opened takes none of it.
#[kithara::test]
fn a_standing_modal_keeps_a_commit_from_the_field_under_it() {
    use iced::keyboard::key::Code;

    let field = with_retained(Holds::SMALL, Page::default(), |ui| {
        ui.rect_of("demo/query")
            .unwrap_or_else(|| panic!("the search field must be laid out"))
    });
    let query = |text: &str| UiEvent::Write {
        key: "fixture.query".to_owned(),
        value: WriteValue::Text(text.to_owned()),
    };
    let focus = [Step::Click(centre(field)), Step::Commit("zz")];
    let [retained, immediate] = both(Holds::Nothing, false, &focus);
    assert_eq!(
        retained,
        [query("zz")],
        "with nothing over it the retained field takes the commit"
    );
    assert_eq!(
        immediate,
        [query("zz")],
        "with nothing over it the immediate field takes the commit"
    );

    let steps = [
        Step::Click(centre(field)),
        Step::Type("a", Code::KeyA),
        Step::Commit("zz"),
        Step::Escape,
    ];
    let [retained, immediate] = both(Holds::SMALL, false, &steps);
    assert_eq!(
        retained,
        [query("a"), trigger("fixture.close")],
        "the retained host"
    );
    assert_eq!(
        immediate,
        [query("a"), trigger("fixture.close")],
        "the immediate host"
    );
}

/// The cursor each host shows once the pointer arrives at a point, retained
/// first: what the retained host asked its window for, and the hand the
/// immediate tree answers with.
fn cursors(holds: Holds, open: bool, at: Pt) -> (String, String) {
    let page = || Page {
        open,
        ..Page::default()
    };
    let retained = with_retained(holds, page(), |ui| {
        pointer(ui, PointerPhase::Move, at);
        format!("{:?}", ui.take_cursor())
    });
    let ui = compiled(holds);
    let mut host = Immediate::mount(page(), &ui, builtin::skin(), WINDOW);
    host.hover_at(at);
    (retained, format!("{:?}", host.hand()))
}

/// Hover over the scrim reaches nothing under it: above the knob and above the
/// search field each host shows the cursor it shows over a quiet part of the
/// scrim, not the one the control beneath asks for.
#[kithara::test]
fn hover_over_the_scrim_shows_nothing_of_the_controls_under_it() {
    let (face, dial) = page_points();
    let field = with_retained(Holds::Nothing, Page::default(), |ui| {
        ui.rect_of("demo/query")
            .map(centre)
            .unwrap_or_else(|| panic!("the search field must be laid out"))
    });
    let quiet = cursors(Holds::SMALL, true, face);
    for (name, at) in [("knob", dial), ("search field", field)] {
        let bare = cursors(Holds::Nothing, false, at);
        assert_ne!(
            bare.0, quiet.0,
            "with nothing over it the retained {name} shows its own cursor"
        );
        assert_ne!(
            bare.1, quiet.1,
            "with nothing over it the immediate {name} shows its own cursor"
        );
        assert_eq!(
            cursors(Holds::SMALL, true, at),
            quiet,
            "the scrim over the {name} shows what it shows anywhere else"
        );
    }
}

/// The wheel over the scrim turns nothing under it: the knob the same notches
/// turn with nothing over it stays where it is.
#[kithara::test]
fn the_wheel_over_the_scrim_turns_nothing_under_it() {
    let (_, dial) = page_points();
    let steps = [Step::Wheel(dial, -2.0)];

    let turned =
        |events: &[UiEvent]| events.iter().any(|event| matches!(event, UiEvent::Write { key, .. } if key == "fixture.dial"));
    let [retained, immediate] = both(Holds::Nothing, false, &steps);
    assert!(turned(&retained), "the bare retained knob turns: {retained:?}");
    assert!(turned(&immediate), "the bare immediate knob turns: {immediate:?}");

    let [retained, immediate] = both(Holds::SMALL, true, &steps);
    assert_eq!(retained, [], "the retained host");
    assert_eq!(immediate, [], "the immediate host");
}

/// A finger on the scrim closes the modal as a press there does, and a finger
/// inside the content does not. The retained host hears a touch as a press.
#[kithara::test]
fn a_touch_on_the_scrim_closes_the_modal_on_the_immediate_host() {
    let (face, _) = page_points();
    let inside = with_retained(Holds::SMALL, Page::open(), |ui| {
        ui.rect_of("demo/inside")
            .map(centre)
            .unwrap_or_else(|| panic!("the modal content must be laid out"))
    });
    let ui = compiled(Holds::SMALL);
    let mut host = Immediate::mount(Page::open(), &ui, builtin::skin(), WINDOW);

    host.touch_at(inside);
    assert_eq!(host.app().published, [], "a touch inside belongs to the content");
    host.touch_at(face);
    assert_eq!(host.app().published, [trigger("fixture.close")]);
}
