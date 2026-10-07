use std::rc::Rc;

use masonry::{
    accesskit::{Node as AccessNode, Role},
    core::{
        AccessCtx, BoxConstraints, ChildrenIds, EventCtx, LayoutCtx, NewWidget, PaintCtx,
        PointerEvent, PropertiesMut, PropertiesRef, QueryCtx, RegisterCtx, Widget, WidgetId,
        WidgetPod, WidgetRef, find_widget_under_pointer,
    },
    kurbo::{Affine, Point, Rect as MasonryRect, Size as MasonrySize, Vec2},
    vello::Scene,
};
use num_traits::cast::AsPrimitive;
use tracing::{Span, trace_span};

use crate::{
    backends::{VelloBackend, paint_color},
    draw::{DrawList, DrawListBuilder, Rect, replay},
    render::{
        ModalChrome, Skin,
        masonry::{custom::HostAction, flex::box_constraints, node::Node, popover::PopoverState},
    },
    solve,
    solve::{Limits, Size},
};

/// The layer a modal stands in: a scrim over the whole window and the framed
/// content centred on it, taking every press the content does not.
pub(crate) struct ModalLayer {
    chrome: ModalChrome,
    state: Rc<PopoverState>,
    declared: Size<solve::Length>,
    child: WidgetPod<Node>,
    /// What the layer draws under its content and over it, laid out with it.
    under: [DrawList; 2],
    over: DrawList,
}

impl ModalLayer {
    pub(crate) fn new(
        content: NewWidget<Node>,
        declared: Size<solve::Length>,
        state: Rc<PopoverState>,
        skin: &Skin,
    ) -> Self {
        Self {
            declared,
            state,
            chrome: ModalChrome::new(skin),
            child: content.to_pod(),
            under: [DrawList::default(), DrawList::default()],
            over: DrawList::default(),
        }
    }

    /// Draws the scrim over the window, the framed surface under the content
    /// and the corner ticks over it.
    fn draw(&mut self, viewport: MasonrySize, surface: Rect) {
        let chrome = self.chrome;
        let mut scrim = DrawListBuilder::default();
        scrim.fill_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: viewport.width.as_(),
                h: viewport.height.as_(),
            },
            chrome.scrim,
        );
        let inset = chrome.border_width / 2.0;
        let mut frame = DrawListBuilder::default();
        frame.fill_rounded_rect(surface, chrome.radius, chrome.background);
        frame.stroke_rounded_rect(
            Rect {
                x: surface.x + inset,
                y: surface.y + inset,
                w: surface.w - chrome.border_width,
                h: surface.h - chrome.border_width,
            },
            chrome.radius,
            chrome.border,
            chrome.border_width,
        );
        let mut ticks = DrawListBuilder::default();
        for tick in chrome.ticks(surface) {
            ticks.fill_rect(tick, chrome.tick);
        }
        self.under = [scrim.finish(), frame.finish()];
        self.over = ticks.finish();
    }
}

fn masonry_rect(rect: Rect) -> MasonryRect {
    MasonryRect::new(
        f64::from(rect.x),
        f64::from(rect.y),
        f64::from(rect.x + rect.w),
        f64::from(rect.y + rect.h),
    )
}

impl Widget for ModalLayer {
    type Action = HostAction;

    fn accepts_pointer_interaction(&self) -> bool {
        true
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut AccessNode,
    ) {
    }

    fn accessibility_role(&self) -> Role {
        Role::Dialog
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }

    fn find_widget_under_pointer<'ctx>(
        &'ctx self,
        ctx: QueryCtx<'ctx>,
        pos: Point,
    ) -> Option<WidgetRef<'ctx, dyn Widget>> {
        self.state.standing()?;
        find_widget_under_pointer(self, ctx, pos)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        constraints: &BoxConstraints,
    ) -> MasonrySize {
        let viewport = constraints.max();
        let standing = self.state.standing().is_some();
        ctx.set_stashed(&mut self.child, !standing);
        if !standing {
            self.state.stand(MasonryRect::ZERO);
            return viewport;
        }
        let window = Size::new(viewport.width.as_(), viewport.height.as_());
        let limits = Limits::new(Size::ZERO, self.chrome.room(window));
        Node::set_child_limits(ctx, &mut self.child, limits);
        let measured = ctx.run_layout(&mut self.child, &box_constraints(limits));
        let content = limits.resolve(
            self.declared.width,
            self.declared.height,
            Size::new(measured.width.as_(), measured.height.as_()),
        );
        let exact = Limits::new(content, content);
        Node::set_child_limits(ctx, &mut self.child, exact);
        ctx.run_layout(
            &mut self.child,
            &BoxConstraints::tight(MasonrySize::new(
                f64::from(content.width),
                f64::from(content.height),
            )),
        );
        let surface = self.chrome.surface(content, window);
        let at = self.chrome.content(surface);
        ctx.place_child(
            &mut self.child,
            Point::new(f64::from(at.x), f64::from(at.y)),
        );
        self.state.stand(masonry_rect(surface));
        self.draw(viewport, surface);
        viewport
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("KitharaModalLayer", id = id.trace())
    }

    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        _event: &PointerEvent,
    ) {
        ctx.set_handled();
    }

    fn paint(&mut self, _ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, scene: &mut Scene) {
        if self.state.standing().is_none() {
            return;
        }
        let chrome = self.chrome;
        let [scrim, frame] = &self.under;
        replay(scrim, &mut VelloBackend::new(scene));
        let surface = self.state.surface();
        let offset = Vec2::new(
            f64::from(chrome.shadow_offset.x),
            f64::from(chrome.shadow_offset.y),
        );
        scene.draw_blurred_rounded_rect(
            Affine::IDENTITY,
            surface + offset,
            paint_color(chrome.shadow),
            f64::from(chrome.radius),
            f64::from(chrome.blur / 2.0),
        );
        replay(frame, &mut VelloBackend::new(scene));
    }

    fn post_paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        scene: &mut Scene,
    ) {
        if self.state.standing().is_none() {
            return;
        }
        replay(&self.over, &mut VelloBackend::new(scene));
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }
}
