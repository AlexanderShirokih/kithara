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
    render::{ReadValue, Reads, Scope, Skin, UiEvent, WindowCommand, WindowEdge, WriteValue},
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

/// A modal whose content is a header that shuts it above a list taller
/// than the box it scrolls in.
const LISTING: &str = r#"Modal(id: "settings", open: Model(id: "fixture.open"),
    close: Command(id: "fixture.close"),
    content: Column(id: "surface", size: (w: Fixed(200.0), h: Fixed(100.0)),
        gap: 0.0, pad: 0.0, children: [
            Pressable(id: "header", press: Command(id: "fixture.shut"),
                child: Spacer(id: "header-face", size: Some((w: Fill, h: Fixed(20.0))))),
            Scroll(id: "list", size: (w: Fill, h: Fixed(80.0)),
                child: Column(gap: 0.0, pad: 0.0, children: [
                    Pressable(id: "row0", press: Command(id: "fixture.row0"),
                        child: Spacer(id: "row0-face", size: Some((w: Fill, h: Fixed(40.0))))),
                    Pressable(id: "row1", press: Command(id: "fixture.row1"),
                        child: Spacer(id: "row1-face", size: Some((w: Fill, h: Fixed(40.0))))),
                    Pressable(id: "row2", press: Command(id: "fixture.row2"),
                        child: Spacer(id: "row2-face", size: Some((w: Fill, h: Fixed(40.0))))),
                    Pressable(id: "row3", press: Command(id: "fixture.row3"),
                        child: Spacer(id: "row3-face", size: Some((w: Fill, h: Fixed(40.0))))),
                    Pressable(id: "row4", press: Command(id: "fixture.row4"),
                        child: Spacer(id: "row4-face", size: Some((w: Fill, h: Fixed(40.0))))),
                ])),
        ])),"#;

/// The listing modal with a button between the header and the list, the kind
/// of control an engine drives in a hosted module.
fn hosted_listing() -> String {
    LISTING
        .replace("h: Fixed(100.0)", "h: Fixed(126.0)")
        .replace(
            "            Scroll(id: \"list\"",
            "            Button(id: \"save\", label: \"SAVE\", write: Command(id: \"fixture.save\"),\n                size: (w: Fill, h: Fixed(26.0))),\n            Scroll(id: \"list\"",
        )
}

/// What the page holds: no modal, a modal whose content asks for a size, or
/// the listing modal, the last in a module whose input an engine owns.
#[derive(Clone, Copy)]
enum Holds {
    Nothing,
    Modal(f32, f32),
    Listing,
    HostedListing,
}

impl Holds {
    const SMALL: Self = Self::Modal(100.0, 60.0);
    const OVERSIZE: Self = Self::Modal(600.0, 400.0);

    fn documents(self) -> MemResolver {
        let modal = match self {
            Self::Nothing => String::new(),
            Self::Modal(width, height) => modal(width, height),
            Self::Listing => LISTING.to_owned(),
            Self::HostedListing => hosted_listing(),
        };
        let module = match self {
            Self::HostedListing => "app-bar",
            Self::Nothing | Self::Modal(..) | Self::Listing => "page",
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
                r#"(schema: "kithara.module", version: 1, id: "{module}", chrome: Plain,
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
    pub(super) const BORDER: f32 = 1.0;
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
            (
                EndpointCategory::Command,
                "fixture.close" | "fixture.page" | "fixture.pick" | "fixture.shut" | "fixture.save"
                | "fixture.burger" | "fixture.row0"
                | "fixture.row1" | "fixture.row2" | "fixture.row3" | "fixture.row4",
            ) => Some(&self.trigger),
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
    assert_eq!(quad.border.width, look::BORDER);
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

/// Whether the retained picture traces `rect` as one closed outline, corner
/// after corner from the top left, in window coordinates.
fn outlines(path_data: &[u32], rect: Rect) -> bool {
    let (left, top) = (rect.x, rect.y);
    let (right, bottom) = (rect.x + rect.w, rect.y + rect.h);
    let corners = [
        left, top, right, top, right, bottom, left, bottom, left, top,
    ]
    .map(f32::to_bits);
    path_data
        .windows(corners.len())
        .any(|traced| traced == corners)
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
        let inset = look::BORDER / 2.0;
        let frame = Rect {
            x: surface.x + inset,
            y: surface.y + inset,
            w: surface.w - look::BORDER,
            h: surface.h - look::BORDER,
        };
        assert!(
            outlines(&encoding.path_data, surface),
            "the surface must fill {surface:?}"
        );
        assert!(
            outlines(&encoding.path_data, frame),
            "the frame must stroke {frame:?}, half its width inside the surface"
        );
        for tick in ticks(surface) {
            assert!(
                outlines(&encoding.path_data, tick),
                "no corner tick at {tick:?}"
            );
        }
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

/// The header's press publishes the header's own write and not the close, and
/// the wheel over the list scrolls it: the row under the list's top edge
/// after the notches is a later row than the one standing there before.
#[kithara::test]
fn a_header_press_and_a_wheel_inside_the_modal_reach_its_content() {
    let (header, top) = with_retained(Holds::Listing, Page::open(), |ui| {
        let header = ui
            .rect_of("demo/header-face")
            .unwrap_or_else(|| panic!("the header must be laid out"));
        let first = ui
            .rect_of("demo/row0-face")
            .unwrap_or_else(|| panic!("the list must be laid out"));
        let top = Pt {
            x: first.x + first.w / 2.0,
            y: first.y + 10.0,
        };
        (centre(header), top)
    });

    let [retained, immediate] = both(Holds::Listing, true, &[Step::Click(header)]);
    assert_eq!(retained, [trigger("fixture.shut")], "the retained host");
    assert_eq!(immediate, [trigger("fixture.shut")], "the immediate host");

    let [retained, immediate] = both(Holds::Listing, true, &[Step::Click(top)]);
    assert_eq!(
        retained,
        [trigger("fixture.row0")],
        "the unscrolled retained list"
    );
    assert_eq!(
        immediate,
        [trigger("fixture.row0")],
        "the unscrolled immediate list"
    );

    let steps = [Step::Wheel(top, -2.0), Step::Click(top)];
    let [retained, immediate] = both(Holds::Listing, true, &steps);
    for (host, events) in [("retained", &retained), ("immediate", &immediate)] {
        assert!(
            matches!(events.as_slice(), [UiEvent::Write { key, .. }]
                if key.starts_with("fixture.row") && key != "fixture.row0"),
            "the {host} list must scroll under the wheel: {events:?}"
        );
    }
    assert_eq!(retained, immediate, "both hosts scroll the list alike");
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

    let turned = |events: &[UiEvent]| {
        events
            .iter()
            .any(|event| matches!(event, UiEvent::Write { key, .. } if key == "fixture.dial"))
    };
    let [retained, immediate] = both(Holds::Nothing, false, &steps);
    assert!(
        turned(&retained),
        "the bare retained knob turns: {retained:?}"
    );
    assert!(
        turned(&immediate),
        "the bare immediate knob turns: {immediate:?}"
    );

    let [retained, immediate] = both(Holds::SMALL, true, &steps);
    assert_eq!(retained, [], "the retained host");
    assert_eq!(immediate, [], "the immediate host");
}

/// A finger on the scrim closes the modal as a press there does, and a finger
/// inside the content does not.
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
    assert_eq!(
        host.app().published,
        [],
        "a touch inside belongs to the content"
    );
    host.touch_at(face);
    assert_eq!(host.app().published, [trigger("fixture.close")]);
}

/// Where a modal stands in the strip of the flow page.
#[derive(Clone, Copy, Debug)]
enum Among {
    Nowhere,
    Between,
    Last,
}

/// A row of two boxes ten apart, `{between}` and `{last}` naming where a modal
/// stands among them, and a third box right after the row: the row takes the
/// room its boxes and gaps need, so a gap the modal charged would move the
/// box after it.
const FLOW: &str = r#"Column(size: (w: Fill, h: Fill), gap: 0.0, pad: 0.0, align: Start, children: [
    Row(id: "line", gap: 0.0, pad: 0.0, align: Start, children: [
        Row(id: "strip", size: (w: Shrink, h: Fixed(20.0)), gap: 10.0, pad: 0.0, align: Start,
            children: [
            Row(id: "a", size: (w: Fixed(20.0), h: Fixed(20.0)), children: []),
            {between}
            Row(id: "b", size: (w: Fixed(20.0), h: Fixed(20.0)), background: Danger,
                children: [Spacer(id: "b-face", size: Some((w: Fill, h: Fill)))]),
            {last}
        ]),
        Row(id: "c", size: (w: Fixed(20.0), h: Fixed(20.0)), background: Success,
            children: [Spacer(id: "c-face", size: Some((w: Fill, h: Fill)))]),
    ]),
])"#;

fn flow(among: Among) -> MemResolver {
    let modal = modal(100.0, 60.0);
    let (between, last) = match among {
        Among::Nowhere => ("", ""),
        Among::Between => (modal.as_str(), ""),
        Among::Last => ("", modal.as_str()),
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
            r#"(schema: "kithara.module", version: 1, id: "page", chrome: Plain, root: {})"#,
            FLOW.replace("{between}", between).replace("{last}", last)
        ),
    );
    resolver
}

/// Where the two boxes that follow the modal stand on each host, retained
/// first.
fn flow_boxes(among: Among, open: bool) -> [[Rect; 2]; 2] {
    let page = || Page {
        open,
        ..Page::default()
    };
    let endpoints = Endpoints::default();
    let resolver = flow(among);
    let ui = Ui::new(
        page(),
        Config::builder()
            .endpoints(&endpoints)
            .resolver(&resolver)
            .text(builtin::text_doc())
            .build(),
        WINDOW,
        1.0,
    )
    .unwrap_or_else(|error| panic!("the flow page must mount on the retained host: {error}"));
    let laid = |path: &str| {
        ui.rect_of(path)
            .unwrap_or_else(|| panic!("{path} must be laid out"))
    };
    let retained = [laid("demo/b-face"), laid("demo/c-face")];

    let compiled = compile(
        "page.klayout.ron",
        &resolver,
        &endpoints,
        builtin::skin_doc(),
        builtin::text_doc(),
        &UiConfig::default(),
        &view::EMPTY,
    )
    .unwrap_or_else(|error| panic!("the flow page must compile: {error}"));
    let quads = Immediate::mount(page(), &compiled, builtin::skin(), WINDOW).quads();
    let filled_with = |role_: ColorRole| {
        quads
            .iter()
            .find(|(_, _, background)| *background == Background::Color(iced_color(role(role_))))
            .map(|(_, quad, _)| Rect {
                x: quad.bounds.x,
                y: quad.bounds.y,
                w: quad.bounds.width,
                h: quad.bounds.height,
            })
            .unwrap_or_else(|| panic!("no box filled with {role_:?} in {quads:#?}"))
    };
    let immediate = [
        filled_with(ColorRole::Danger),
        filled_with(ColorRole::Success),
    ];
    [retained, immediate]
}

/// A modal takes no room in a flow and charges it no gap, shown or shut: the
/// boxes after it stand where they stand with no modal there at all.
#[kithara::test]
fn a_modal_takes_no_room_and_no_gap_in_a_flow() {
    let [retained, immediate] = flow_boxes(Among::Nowhere, false);
    assert_eq!(retained, immediate, "the hosts agree on the bare flow");
    for among in [Among::Between, Among::Last] {
        for open in [false, true] {
            let [shown_retained, shown_immediate] = flow_boxes(among, open);
            assert_eq!(
                shown_retained, retained,
                "the retained host, the modal {among:?}, open {open}"
            );
            assert_eq!(
                shown_immediate, immediate,
                "the immediate host, the modal {among:?}, open {open}"
            );
        }
    }
}

/// A window that resizes by its own edges, with the modal over the page.
fn chrome() -> MemResolver {
    let mut resolver = MemResolver::default();
    resolver.insert(
        "page.klayout.ron",
        r#"(schema: "kithara.layout", version: 1, id: "page", resize_edges: true,
            root: Module(instance: "demo", source: "page.kmodule.ron", size: (w: Fill, h: Fill)))"#,
    );
    resolver.insert(
        "page.kmodule.ron",
        &format!(
            r#"(schema: "kithara.module", version: 1, id: "page", chrome: Plain,
                root: Column(size: (w: Fill, h: Fill), gap: 0.0, pad: 0.0, children: [
                    {}
                    Pressable(id: "page", press: Command(id: "fixture.page"),
                        child: Spacer(id: "page-face", size: Some((w: Fill, h: Fill)))),
                ]))"#,
            modal(100.0, 60.0)
        ),
    );
    resolver
}

/// The window's resize edges answer before the modal: a press on one under
/// the scrim resizes the window and leaves the modal standing.
#[kithara::test]
fn a_resize_edge_answers_before_the_modal() {
    let edge = Pt { x: 1.0, y: 250.0 };
    let commands = [WindowCommand::Resize(WindowEdge::West)];

    let endpoints = Endpoints::default();
    let resolver = chrome();
    let mut ui = Ui::new(
        Page::open(),
        Config::builder()
            .endpoints(&endpoints)
            .resolver(&resolver)
            .text(builtin::text_doc())
            .build(),
        WINDOW,
        1.0,
    )
    .unwrap_or_else(|error| panic!("the chrome page must mount on the retained host: {error}"));
    play_retained(&mut ui, &[Step::Click(edge)]);
    assert_eq!(ui.take_window_commands(), commands, "the retained window");
    assert_eq!(
        ui.app().published,
        commands.map(UiEvent::Window),
        "the retained window, with the modal left standing"
    );

    let compiled = compile(
        "page.klayout.ron",
        &resolver,
        &endpoints,
        builtin::skin_doc(),
        builtin::text_doc(),
        &UiConfig::default(),
        &view::EMPTY,
    )
    .unwrap_or_else(|error| panic!("the chrome page must compile: {error}"));
    let mut host = Immediate::mount(Page::open(), &compiled, builtin::skin(), WINDOW);
    host.click_at(edge);
    assert_eq!(
        host.app().published,
        commands.map(UiEvent::Window),
        "the immediate window, with the modal left standing"
    );
}

/// Where the modal stands in the document against the title strip.
#[derive(Clone, Copy, Debug)]
enum Titled {
    Before,
    After,
}

/// A page whose document draws its own title strip: window controls and a
/// title bar along the top, the modal before or after them.
fn titled(order: Titled) -> MemResolver {
    let strip = r#"Row(size: (w: Fill, h: Fixed(32.0)), gap: 0.0, pad: 0.0, children: [
            WindowControls(id: "controls", style: Standard),
            TitleBar(id: "title", label: "KITHARA"),
        ]),"#;
    let (first, second) = match order {
        Titled::Before => (modal(100.0, 60.0), strip.to_owned()),
        Titled::After => (strip.to_owned(), modal(100.0, 60.0)),
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
                root: Column(size: (w: Fill, h: Fill), gap: 0.0, pad: 0.0, children: [
                    {first}
                    {second}
                    Pressable(id: "page", press: Command(id: "fixture.page"),
                        child: Spacer(id: "page-face", size: Some((w: Fill, h: Fill)))),
                ]))"#
        ),
    );
    resolver
}

/// A title bar and window controls drawn by the document lie under the scrim
/// whatever their place in it: a press on one closes the modal and moves,
/// shrinks or closes no window.
#[kithara::test]
fn a_press_on_a_drawn_title_strip_closes_the_modal_and_reaches_no_window() {
    let targets = [
        ("title bar", Pt { x: 300.0, y: 16.0 }),
        ("minimise cell", Pt { x: 17.5, y: 16.0 }),
        ("close cell", Pt { x: 62.5, y: 16.0 }),
    ];
    let closed = [trigger("fixture.close")];
    let endpoints = Endpoints::default();
    for order in [Titled::Before, Titled::After] {
        let resolver = titled(order);
        let compiled = compile(
            "page.klayout.ron",
            &resolver,
            &endpoints,
            builtin::skin_doc(),
            builtin::text_doc(),
            &UiConfig::default(),
            &view::EMPTY,
        )
        .unwrap_or_else(|error| panic!("the titled page must compile: {error}"));
        for (target, at) in targets {
            let mut ui = Ui::new(
                Page::open(),
                Config::builder()
                    .endpoints(&endpoints)
                    .resolver(&resolver)
                    .text(builtin::text_doc())
                    .build(),
                WINDOW,
                1.0,
            )
            .unwrap_or_else(|error| {
                panic!("the titled page must mount on the retained host: {error}")
            });
            play_retained(&mut ui, &[Step::Click(at)]);
            assert_eq!(
                ui.take_window_commands(),
                [],
                "the retained {target}, modal {order:?} it"
            );
            assert_eq!(
                ui.app().published,
                closed,
                "the retained {target}, modal {order:?} it"
            );

            let mut host = Immediate::mount(Page::open(), &compiled, builtin::skin(), WINDOW);
            host.click_at(at);
            assert_eq!(
                host.app().published,
                closed,
                "the immediate {target}, modal {order:?} it"
            );
        }
    }
}

/// Mounts the titled page with the modal standing on the retained host.
fn with_titled<R>(order: Titled, check: impl FnOnce(&mut Ui<'_, Page>) -> R) -> R {
    let endpoints = Endpoints::default();
    let resolver = titled(order);
    let mut ui = Ui::new(
        Page::open(),
        Config::builder()
            .endpoints(&endpoints)
            .resolver(&resolver)
            .text(builtin::text_doc())
            .build(),
        WINDOW,
        1.0,
    )
    .unwrap_or_else(|error| panic!("the titled page must mount on the retained host: {error}"));
    check(&mut ui)
}

/// The cursor each host shows over a point of the titled page with the modal
/// standing, retained first.
fn titled_cursors(order: Titled, at: Pt) -> (String, String) {
    let retained = with_titled(order, |ui| {
        pointer(ui, PointerPhase::Move, at);
        format!("{:?}", ui.take_cursor())
    });
    let compiled = compile(
        "page.klayout.ron",
        &titled(order),
        &Endpoints::default(),
        builtin::skin_doc(),
        builtin::text_doc(),
        &UiConfig::default(),
        &view::EMPTY,
    )
    .unwrap_or_else(|error| panic!("the titled page must compile: {error}"));
    let mut host = Immediate::mount(Page::open(), &compiled, builtin::skin(), WINDOW);
    host.hover_at(at);
    (retained, format!("{:?}", host.hand()))
}

/// A drawn title strip lies under the scrim in the picture and under the
/// pointer whatever its place in the document: the retained host paints the
/// strip before the scrim, and hover over the strip shows the cursor a quiet
/// part of the scrim shows.
#[kithara::test]
fn a_drawn_title_strip_paints_and_hovers_under_the_modal_whatever_its_order() {
    let title_ink = packed(role(builtin::skin().window.titlebar_text.color));
    let scrim = packed(faded(ColorRole::BgDeep, look::SCRIM_ALPHA));
    for order in [Titled::Before, Titled::After] {
        let drawn = with_titled(order, |ui| {
            ui.scene()
                .unwrap_or_else(|error| panic!("the titled page must draw: {error}"))
                .encoding()
                .draw_data
                .clone()
        });
        let title = drawn.iter().rposition(|word| *word == title_ink);
        let covered = drawn.iter().position(|word| *word == scrim);
        assert!(
            matches!((title, covered), (Some(title), Some(covered)) if title < covered),
            "the retained title must be painted before the scrim, modal {order:?} it: title at \
             {title:?}, scrim at {covered:?}"
        );

        let quiet = titled_cursors(order, Pt { x: 300.0, y: 250.0 });
        for (target, at) in [
            ("title bar", Pt { x: 300.0, y: 16.0 }),
            ("minimise cell", Pt { x: 17.5, y: 16.0 }),
            ("close cell", Pt { x: 62.5, y: 16.0 }),
        ] {
            assert_eq!(
                titled_cursors(order, at),
                quiet,
                "hover over the {target} under the scrim, modal {order:?} it"
            );
        }
    }
}

/// A modal inside a module whose input an engine owns keeps the same input as
/// one in a plain module: the header and the button an engine drives publish
/// their own writes, the wheel scrolls the list, and a press, a drag or the
/// wheel on the scrim writes the close or nothing and reaches nothing under it.
#[kithara::test]
fn a_modal_in_a_hosted_module_hears_its_content_and_keeps_the_page() {
    let (face, dial) = page_points();
    let (header, save, top) = with_retained(Holds::HostedListing, Page::open(), |ui| {
        let header = ui
            .rect_of("demo/header-face")
            .unwrap_or_else(|| panic!("the header must be laid out"));
        let save = ui
            .rect_of("demo/save")
            .unwrap_or_else(|| panic!("the button must be laid out"));
        let first = ui
            .rect_of("demo/row0-face")
            .unwrap_or_else(|| panic!("the list must be laid out"));
        let top = Pt {
            x: first.x + first.w / 2.0,
            y: first.y + 10.0,
        };
        (centre(header), centre(save), top)
    });

    let [retained, immediate] = both(Holds::HostedListing, true, &[Step::Click(header)]);
    assert_eq!(retained, [trigger("fixture.shut")], "the retained header");
    assert_eq!(immediate, [trigger("fixture.shut")], "the immediate header");

    let [retained, immediate] = both(Holds::HostedListing, true, &[Step::Click(save)]);
    assert_eq!(retained, [trigger("fixture.save")], "the retained button");
    assert_eq!(immediate, [trigger("fixture.save")], "the immediate button");

    let steps = [Step::Wheel(top, -2.0), Step::Click(top)];
    let [retained, immediate] = both(Holds::HostedListing, true, &steps);
    for (host, events) in [("retained", &retained), ("immediate", &immediate)] {
        assert!(
            matches!(events.as_slice(), [UiEvent::Write { key, .. }]
                if key.starts_with("fixture.row") && key != "fixture.row0"),
            "the {host} list must scroll under the wheel: {events:?}"
        );
    }
    assert_eq!(retained, immediate, "both hosts scroll the list alike");

    let up = Pt {
        x: dial.x,
        y: dial.y - 20.0,
    };
    let steps = [
        Step::Click(face),
        Step::Drag(dial, up),
        Step::Wheel(dial, -2.0),
    ];
    let [retained, immediate] = both(Holds::HostedListing, false, &steps);
    for (host, events) in [("retained", &retained), ("immediate", &immediate)] {
        assert!(
            events.contains(&trigger("fixture.page"))
                && events
                    .iter()
                    .any(|event| matches!(event, UiEvent::Write { key, .. } if key == "fixture.dial")),
            "with the modal shut the {host} page hears the press and the drag: {events:?}"
        );
    }
    let closed = vec![trigger("fixture.close"), trigger("fixture.close")];
    let [retained, immediate] = both(Holds::HostedListing, true, &steps);
    assert_eq!(retained, closed, "the retained scrim");
    assert_eq!(immediate, closed, "the immediate scrim");
}

/// A page with the modal first and an open menu after it, the menu holding the
/// pressable `pick` row: both read the same flag, so both stand at once.
fn popped() -> MemResolver {
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
                root: Column(size: (w: Fill, h: Fill), gap: 0.0, pad: 0.0, children: [
                    {}
                    Popover(id: "menu", open: Model(id: "fixture.open"), align: Start,
                        anchor: Pressable(id: "burger", press: Command(id: "fixture.burger"),
                            child: Spacer(id: "anchor", size: Some((w: Fixed(40.0), h: Fixed(20.0))))),
                        content: Pressable(id: "menu-row", press: Command(id: "fixture.pick"),
                            child: Spacer(id: "menu-face", size: Some((w: Fixed(100.0), h: Fixed(60.0)))))),
                    Pressable(id: "page", press: Command(id: "fixture.page"),
                        child: Spacer(id: "page-face", size: Some((w: Fill, h: Fill)))),
                ]))"#,
            modal(100.0, 60.0)
        ),
    );
    resolver
}

/// A menu standing after the modal in the document lies under it like the
/// rest of the page: a press on the menu's row closes the modal and reaches
/// nothing of the menu, and hover over the row shows what the scrim shows.
#[kithara::test]
fn a_menu_after_the_modal_lies_under_it() {
    let endpoints = Endpoints::default();
    let resolver = popped();
    let retained = |hover: Option<Pt>, steps: &[Step]| {
        let mut ui = Ui::new(
            Page::open(),
            Config::builder()
                .endpoints(&endpoints)
                .resolver(&resolver)
                .text(builtin::text_doc())
                .build(),
            WINDOW,
            1.0,
        )
        .unwrap_or_else(|error| panic!("the menu page must mount on the retained host: {error}"));
        let row = ui
            .rect_of("demo/menu-face")
            .map(centre)
            .unwrap_or_else(|| panic!("the open menu must be laid out"));
        if let Some(at) = hover {
            pointer(&mut ui, PointerPhase::Move, at);
        }
        let cursor = format!("{:?}", ui.take_cursor());
        play_retained(&mut ui, steps);
        (row, ui.app().published.clone(), cursor)
    };
    let (row, _, _) = retained(None, &[]);
    let quiet = Pt { x: 300.0, y: 250.0 };
    let compiled = compile(
        "page.klayout.ron",
        &resolver,
        &endpoints,
        builtin::skin_doc(),
        builtin::text_doc(),
        &UiConfig::default(),
        &view::EMPTY,
    )
    .unwrap_or_else(|error| panic!("the menu page must compile: {error}"));
    let immediate = |at: Pt| {
        let mut host = Immediate::mount(Page::open(), &compiled, builtin::skin(), WINDOW);
        host.hover_at(at);
        let hand = format!("{:?}", host.hand());
        host.click_at(at);
        (host.app().published.clone(), hand)
    };

    let (_, pressed, _) = retained(None, &[Step::Click(row)]);
    assert_eq!(pressed, [trigger("fixture.close")], "the retained menu row");
    let (pressed, _) = immediate(row);
    assert_eq!(pressed, [trigger("fixture.close")], "the immediate menu row");

    let (_, _, over_row) = retained(Some(row), &[]);
    let (_, _, over_scrim) = retained(Some(quiet), &[]);
    assert_eq!(over_row, over_scrim, "the retained hover over the menu row");
    assert_eq!(
        immediate(row).1,
        immediate(quiet).1,
        "the immediate hover over the menu row"
    );
}
