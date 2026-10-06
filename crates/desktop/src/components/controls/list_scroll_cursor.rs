//! Keeps the mouse cursor stable for one vertical list-scroll gesture.
//!
//! Pinned GPUI (`cc053a4a`) calls `Platform::set_cursor_style` from
//! `Window::reset_cursor_style` at the end of every frame, and again when the
//! hit test under the pointer changes. On macOS that stores the style and, only
//! when it differs, calls AppKit `invalidateCursorRectsForView`. Enabled
//! gpui-base inputs paint `cursor_text` (I-beam). The gaps between Environment
//! Manager rows do not, so a stationary pointer alternates I-beam and arrow as
//! rows move, and each flip reaches AppKit.
//!
//! App code cannot read the resolved style (`Frame::cursor_style` is
//! crate-private) or skip the platform update. A cursor painted for a list
//! hitbox during the gesture does override the inputs, because resolution walks
//! paint order back to front and the first hovered request wins. The test
//! platform stores the style in a private field and never exposes it.

use std::time::{Duration, Instant};

use gpui::{
    App, CursorStyle, EntityId, Global, HitboxBehavior, Pixels, Point, ScrollWheelEvent,
    Styled as _, Task, TouchPhase, Window, canvas, px,
};

/// How long a wheel with no touch phase, or trackpad momentum after the finger
/// lifts, may go quiet before the cursor is allowed to change again.
///
/// This GPUI revision maps a trackpad's `momentumPhase` to ordinary
/// `TouchPhase::Moved` events (it only reads `NSEvent.phase`). Ending the hold
/// on `Ended` alone would drop it for the inertial frames that follow.
pub(crate) const LIST_SCROLL_CURSOR_IDLE: Duration = Duration::from_millis(150);

#[derive(Default)]
struct ListScrollCursorHold {
    held: Option<CursorStyle>,
    release_at: Option<Instant>,
    waiter: Option<Task<()>>,
    waiter_running: bool,
    waiter_finished: bool,
    #[cfg(test)]
    published: Vec<Option<CursorStyle>>,
    #[cfg(test)]
    seen_during_hold: Vec<CursorStyle>,
}

impl Global for ListScrollCursorHold {}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListScrollCursorTrace {
    pub(crate) published: Vec<Option<CursorStyle>>,
    pub(crate) seen_during_hold: Vec<CursorStyle>,
}

#[cfg(test)]
pub(crate) fn list_scroll_cursor_trace(cx: &App) -> ListScrollCursorTrace {
    cx.try_global::<ListScrollCursorHold>()
        .map(|hold| ListScrollCursorTrace {
            published: hold.published.clone(),
            seen_during_hold: hold.seen_during_hold.clone(),
        })
        .unwrap_or(ListScrollCursorTrace {
            published: Vec::new(),
            seen_during_hold: Vec::new(),
        })
}

pub(crate) fn list_scroll_cursor_held(cx: &App) -> bool {
    cx.try_global::<ListScrollCursorHold>()
        .is_some_and(|hold| hold.held.is_some())
}

/// Paint the frozen cursor over a scrolling list. The caller places this after
/// the rows so it wins over each field's I-beam while the pointer stays in the
/// list. It is omitted once the gesture ends, and the next frame resolves the
/// cursor for whatever is under the pointer.
pub(crate) fn list_scroll_cursor_overlay() -> impl gpui::IntoElement {
    canvas(
        |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        |_bounds, hitbox, window, cx| {
            if let Some(style) = cx
                .try_global::<ListScrollCursorHold>()
                .and_then(|hold| hold.held)
            {
                window.set_cursor_style(style, &hitbox);
            }
        },
    )
    .absolute()
    .top(px(0.0))
    .right(px(0.0))
    .bottom(px(0.0))
    .left(px(0.0))
}

/// A wheel that missed every text field. Gaps do not paint an I-beam.
pub(crate) fn observe_list_background_scroll(
    event: &ScrollWheelEvent,
    view: EntityId,
    window: &mut Window,
    cx: &mut App,
) {
    note_listed_scroll(
        CursorStyle::Arrow,
        event.delta.precise(),
        event.touch_phase,
        event.delta.pixel_delta(window.line_height()),
        view,
        window,
        cx,
    );
}

pub(super) fn note_listed_scroll(
    style_if_starting: CursorStyle,
    precise: bool,
    phase: TouchPhase,
    delta: Point<Pixels>,
    view: EntityId,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ending) = vertical_list_sample(precise, phase, delta) else {
        return;
    };
    let now = cx.background_executor().now();
    let (start_waiter, started_hold) = {
        let hold = cursor_hold(cx);
        let was_held = hold.held.is_some();
        let start_waiter = observe_sample(hold, style_if_starting, ending, now);
        (start_waiter, !was_held && hold.held.is_some())
    };
    if start_waiter {
        let task = window.spawn(cx, async move |cx| {
            loop {
                let Some(wait) = cx
                    .update(|_, app| {
                        let now = app.background_executor().now();
                        cursor_hold(app)
                            .release_at
                            .map(|deadline| deadline.saturating_duration_since(now))
                            .unwrap_or(Duration::ZERO)
                    })
                    .ok()
                else {
                    return;
                };
                if !wait.is_zero() {
                    cx.background_executor().timer(wait).await;
                    continue;
                }
                let stop = cx
                    .update(|window, app| {
                        let now = app.background_executor().now();
                        let stop = poll_release(cursor_hold(app), now);
                        if stop {
                            window.refresh();
                        }
                        stop
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
            }
        });
        cursor_hold(cx).waiter = Some(task);
    }
    if started_hold {
        cx.notify(view);
    }
}

fn cursor_hold(cx: &mut App) -> &mut ListScrollCursorHold {
    if !cx.has_global::<ListScrollCursorHold>() {
        cx.set_global(ListScrollCursorHold::default());
    }
    cx.global_mut()
}

/// `Some(true)` ends a precise gesture. `Some(false)` continues one. `None`
/// leaves the hold alone (a horizontal wheel belongs to the field).
fn vertical_list_sample(precise: bool, phase: TouchPhase, delta: Point<Pixels>) -> Option<bool> {
    let ending = precise && matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled);
    let vertical =
        delta.x.abs() <= delta.y.abs() && (delta.y != px(0.0) || phase == TouchPhase::Started);
    if vertical || ending {
        Some(ending)
    } else {
        None
    }
}

/// Returns whether a release waiter needs to be started.
fn observe_sample(
    hold: &mut ListScrollCursorHold,
    style_if_starting: CursorStyle,
    ending: bool,
    now: Instant,
) -> bool {
    if ending && hold.held.is_none() {
        return false;
    }
    if hold.held.is_none() {
        hold.held = Some(style_if_starting);
        #[cfg(test)]
        hold.published.push(Some(style_if_starting));
    }
    #[cfg(test)]
    hold.seen_during_hold.push(style_if_starting);
    hold.release_at = Some(now + LIST_SCROLL_CURSOR_IDLE);
    if hold.waiter_finished {
        hold.waiter = None;
        hold.waiter_finished = false;
        hold.waiter_running = false;
    }
    let start_waiter = !hold.waiter_running;
    if start_waiter {
        hold.waiter_running = true;
    }
    start_waiter
}

/// Returns whether the waiter should stop. A deadline that moved forward keeps
/// the same frozen style.
fn poll_release(hold: &mut ListScrollCursorHold, now: Instant) -> bool {
    if hold
        .release_at
        .is_some_and(|deadline| now < deadline && hold.held.is_some())
    {
        return false;
    }
    if hold.held.take().is_some() {
        #[cfg(test)]
        hold.published.push(None);
    }
    hold.release_at = None;
    hold.waiter_running = false;
    hold.waiter_finished = true;
    true
}

#[cfg(test)]
mod tests {
    use super::{
        LIST_SCROLL_CURSOR_IDLE, ListScrollCursorHold, observe_sample, poll_release,
        vertical_list_sample,
    };
    use gpui::{CursorStyle, TouchPhase, point, px};
    use std::time::{Duration, Instant};

    #[test]
    fn horizontal_wheels_do_not_sample_the_list_cursor() {
        let horizontal = point(px(-40.0), px(-6.0));
        assert_eq!(
            vertical_list_sample(true, TouchPhase::Moved, horizontal),
            None
        );
        assert_eq!(
            vertical_list_sample(false, TouchPhase::Moved, horizontal),
            None
        );
        assert_eq!(
            vertical_list_sample(true, TouchPhase::Ended, point(px(0.0), px(-12.0))),
            Some(true)
        );
        assert_eq!(
            vertical_list_sample(false, TouchPhase::Moved, point(px(1.0), px(-20.0))),
            Some(false)
        );
        assert_eq!(
            vertical_list_sample(true, TouchPhase::Started, point(px(0.0), px(0.0))),
            Some(false)
        );
    }

    #[test]
    fn one_gesture_publishes_a_single_style_until_idle() {
        let start = Instant::now();
        let mut hold = ListScrollCursorHold::default();
        assert!(observe_sample(&mut hold, CursorStyle::IBeam, false, start));
        for (at, style) in [
            (Duration::from_millis(16), CursorStyle::Arrow),
            (Duration::from_millis(32), CursorStyle::IBeam),
            (Duration::from_millis(48), CursorStyle::Arrow),
        ] {
            assert!(
                !observe_sample(&mut hold, style, false, start + at),
                "later ticks reuse the waiter started with the gesture"
            );
        }
        assert!(!observe_sample(
            &mut hold,
            CursorStyle::Arrow,
            true,
            start + Duration::from_millis(64)
        ));
        assert_eq!(hold.published, vec![Some(CursorStyle::IBeam)]);
        assert_eq!(
            hold.seen_during_hold,
            vec![
                CursorStyle::IBeam,
                CursorStyle::Arrow,
                CursorStyle::IBeam,
                CursorStyle::Arrow,
                CursorStyle::Arrow,
            ]
        );
        let ended_at = start + Duration::from_millis(64);
        assert!(!poll_release(
            &mut hold,
            ended_at + LIST_SCROLL_CURSOR_IDLE - Duration::from_millis(1)
        ));
        assert_eq!(hold.held, Some(CursorStyle::IBeam));
        assert!(poll_release(&mut hold, ended_at + LIST_SCROLL_CURSOR_IDLE));
        assert_eq!(hold.held, None);
        assert_eq!(
            hold.published,
            vec![Some(CursorStyle::IBeam), None],
            "the gesture releases once so the cursor can be resolved again"
        );
    }

    #[test]
    fn a_phase_end_without_a_gesture_does_not_freeze_the_cursor() {
        let mut hold = ListScrollCursorHold::default();
        assert!(!observe_sample(
            &mut hold,
            CursorStyle::Arrow,
            true,
            Instant::now()
        ));
        assert!(hold.held.is_none());
        assert!(hold.published.is_empty());
    }
}
