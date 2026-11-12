//! Motion that makes the board feel alive: things glide to new places instead of jumping, new
//! things pop in, dragged images lift and swing, and flicked images or boards coast to a stop.
//!
//! None of this changes the board: it only decides where things are *drawn* while they settle.

use super::*;

/// How long a glide to a new place takes.
const GLIDE: Duration = Duration::from_millis(240);
/// How long a new item takes to pop in.
const POP: Duration = Duration::from_millis(320);
/// How long the view takes to fly to a new framing (fit, zoom 100%).
const VIEW: Duration = Duration::from_millis(280);
/// Releasing a drag faster than this (screen points per second) flicks it.
const FLICK_SPEED: f32 = 450.0;
/// How quickly a flick slows down (per second, exponential).
const FRICTION: f32 = 5.5;
/// Strongest tilt while swinging an image around, in radians (about 6°).
const MAX_TILT: f32 = 0.11;

#[derive(Default)]
pub(super) struct Motion {
    /// Each item's world rect last frame, to notice when something moved by itself.
    prev: HashMap<u64, Rect>,
    /// Where each item was last drawn, so a new glide starts from what's on screen.
    shown: HashMap<u64, Rect>,
    glides: HashMap<u64, (Rect, Instant)>,
    pops: HashMap<u64, Instant>,
    /// Per dragged image: (tilt in radians, lift 0..1).
    swing: HashMap<u64, (f32, f32)>,
    flick: Option<Flick>,
    view: Option<(board::View, board::View, Instant)>,
    /// False until the first frame of a board, so loading one doesn't animate everything in.
    started: bool,
    last_frame: Option<Instant>,
}

struct Flick {
    /// Items coasting in world space; empty when the whole board (the view) is coasting.
    items: Vec<u64>,
    /// Screen points per second.
    velocity: Vec2,
}

pub(super) fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Overshoots a little before settling, for a springy pop.
fn ease_out_back(t: f32) -> f32 {
    let c = 1.6;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

fn same_shape(a: &Rect, b: &Rect) -> bool {
    let ratio = |r: &Rect| r.width() / r.height().max(f32::EPSILON);
    (ratio(a) / ratio(b).max(f32::EPSILON) - 1.0).abs() < 0.01
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(a.min.lerp(b.min, t), a.max.lerp(b.max, t))
}

impl CoreBoardApp {
    /// Starts over for a freshly loaded board.
    pub(super) fn reset_motion(&mut self) {
        self.motion = Motion::default();
    }

    /// Notices items that moved or appeared without being dragged, and advances flicks, view
    /// flights and swings. Call once per frame, before handling input and painting.
    pub(super) fn update_motion(&mut self, ctx: &egui::Context, canvas: Rect) {
        let now = Instant::now();
        let dt = self
            .motion
            .last_frame
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.05));
        self.motion.last_frame = Some(now);
        let enabled = self.animations;

        self.advance_flick(dt, canvas);
        if let Some((from, to, start)) = self.motion.view.clone() {
            let t = (now.duration_since(start).as_secs_f32() / VIEW.as_secs_f32()).min(1.0);
            let e = ease_out_cubic(t);
            // Zoom eases in log space, so zooming in and out feel equally smooth.
            let zoom = (from.zoom.ln() + (to.zoom.ln() - from.zoom.ln()) * e).exp();
            let offset = [
                from.offset[0] + (to.offset[0] - from.offset[0]) * e,
                from.offset[1] + (to.offset[1] - from.offset[1]) * e,
            ];
            self.board.view = board::View { offset, zoom };
            if t >= 1.0 {
                self.motion.view = None;
            }
        }

        // Items being moved by hand (or coasting) are drawn where they are; everything else
        // that changed place glides there.
        let held: HashSet<u64> = match &self.drag {
            Drag::Move { start, .. } => start.keys().copied().collect(),
            Drag::Resize { id, .. } => HashSet::from([*id]),
            _ => HashSet::new(),
        };
        let coasting: HashSet<u64> = self
            .motion
            .flick
            .iter()
            .flat_map(|f| f.items.iter().copied())
            .collect();
        let mut current = HashMap::new();
        for item in &self.board.items {
            let rect = Rect::from_min_size(
                pos2(item.pos[0], item.pos[1]),
                vec2(item.size[0], item.size[1]),
            );
            current.insert(item.id, rect);
            if !self.motion.started || !enabled {
                continue;
            }
            match self.motion.prev.get(&item.id) {
                None => {
                    self.motion.pops.insert(item.id, now);
                }
                // A change of shape (crop, rotation) applies at once: gliding it would stretch
                // the picture on the way.
                Some(prev)
                    if *prev != rect
                        && same_shape(prev, &rect)
                        && item.pinned.is_none()
                        && !held.contains(&item.id)
                        && !coasting.contains(&item.id) =>
                {
                    let from = self.motion.shown.get(&item.id).copied().unwrap_or(*prev);
                    self.motion.glides.insert(item.id, (from, now));
                }
                _ => {}
            }
        }
        self.motion.prev = current;
        self.motion.started = true;
        self.motion
            .glides
            .retain(|_, (_, start)| now.duration_since(*start) < GLIDE);
        self.motion
            .pops
            .retain(|_, start| now.duration_since(*start) < POP);

        // Dragged images lift and lean into the motion; released ones settle back.
        let velocity = ctx.input(|i| i.pointer.velocity());
        let lean = (velocity.x * 0.00014).clamp(-MAX_TILT, MAX_TILT);
        let dragged: HashSet<u64> = match &self.drag {
            Drag::Move {
                moved: true, start, ..
            } if enabled => start.keys().copied().collect(),
            _ => HashSet::new(),
        };
        let ease = (dt * 14.0).min(1.0);
        for id in &dragged {
            self.motion.swing.entry(*id).or_insert((0.0, 0.0));
        }
        self.motion.swing.retain(|id, (tilt, lift)| {
            let (want_tilt, want_lift) = if dragged.contains(id) {
                (lean, 1.0)
            } else {
                (0.0, 0.0)
            };
            *tilt += (want_tilt - *tilt) * ease;
            *lift += (want_lift - *lift) * ease;
            dragged.contains(id) || tilt.abs() > 0.002 || *lift > 0.01
        });

        if self.is_moving() {
            ctx.request_repaint();
        }
    }

    fn is_moving(&self) -> bool {
        let m = &self.motion;
        !m.glides.is_empty()
            || !m.pops.is_empty()
            || !m.swing.is_empty()
            || m.flick.is_some()
            || m.view.is_some()
    }

    /// Where to draw an item this frame (screen space), and how opaque: glides, pops and lifts
    /// applied. Hit-testing still uses the item's real place.
    pub(super) fn shown_rect(&mut self, canvas: Rect, item: &Item) -> (Rect, f32) {
        let real = self.item_screen_rect(canvas, item);
        if !self.animations || item.pinned.is_some() {
            return (real, 1.0);
        }
        let now = Instant::now();
        let mut world = Rect::from_min_size(
            pos2(item.pos[0], item.pos[1]),
            vec2(item.size[0], item.size[1]),
        );
        if let Some((from, start)) = self.motion.glides.get(&item.id) {
            let t = now.duration_since(*start).as_secs_f32() / GLIDE.as_secs_f32();
            world = lerp_rect(*from, world, ease_out_cubic(t.min(1.0)));
        }
        self.motion.shown.insert(item.id, world);
        let mut rect = Rect::from_min_max(
            self.to_screen(canvas, world.min),
            self.to_screen(canvas, world.max),
        );
        let mut alpha = 1.0;
        if let Some(start) = self.motion.pops.get(&item.id) {
            let t = (now.duration_since(*start).as_secs_f32() / POP.as_secs_f32()).min(1.0);
            let scale = 0.82 + 0.18 * ease_out_back(t);
            rect = Rect::from_center_size(rect.center(), rect.size() * scale);
            alpha = (t * 2.5).min(1.0);
        }
        if let Some((_, lift)) = self.motion.swing.get(&item.id) {
            rect = Rect::from_center_size(rect.center(), rect.size() * (1.0 + 0.03 * lift));
        }
        (rect, alpha)
    }

    /// The current tilt and lift of a dragged (or settling) image.
    pub(super) fn swing_of(&self, id: u64) -> (f32, f32) {
        self.motion.swing.get(&id).copied().unwrap_or((0.0, 0.0))
    }

    /// Flies the view to `to` (or jumps, with animations off).
    pub(super) fn fly_view(&mut self, to: board::View) {
        if self.animations {
            self.motion.view = Some((self.board.view.clone(), to, Instant::now()));
        } else {
            self.board.view = to;
        }
        self.touch();
    }

    /// Stops a view flight or a flick, e.g. when the user grabs the board again.
    pub(super) fn stop_motion(&mut self) {
        self.motion.view = None;
        self.motion.flick = None;
    }

    /// Lets go of a drag: fast enough, and the items (or the board) keep coasting.
    pub(super) fn release_flick(&mut self, ctx: &egui::Context, items: Vec<u64>) {
        let velocity = ctx.input(|i| i.pointer.velocity());
        if self.animations && velocity.length() > FLICK_SPEED {
            self.motion.flick = Some(Flick { items, velocity });
        }
    }

    fn advance_flick(&mut self, dt: f32, _canvas: Rect) {
        let Some(flick) = &mut self.motion.flick else {
            return;
        };
        let step = flick.velocity * dt;
        flick.velocity *= (-FRICTION * dt).exp();
        let done = flick.velocity.length() < 25.0;
        let zoom = self.board.view.zoom;
        if flick.items.is_empty() {
            self.board.view.offset[0] += step.x;
            self.board.view.offset[1] += step.y;
        } else {
            let ids: HashSet<u64> = flick.items.iter().copied().collect();
            for item in &mut self.board.items {
                if ids.contains(&item.id) {
                    item.pos[0] += step.x / zoom;
                    item.pos[1] += step.y / zoom;
                }
            }
        }
        if done {
            self.motion.flick = None;
        }
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_starts_and_ends_in_place() {
        for ease in [ease_out_cubic, ease_out_back] {
            assert!(ease(0.0).abs() < 1e-6);
            assert!((ease(1.0) - 1.0).abs() < 1e-6);
        }
        // The pop overshoots a little before settling.
        assert!(
            (0..100)
                .map(|i| ease_out_back(i as f32 / 100.0))
                .any(|v| v > 1.02)
        );
    }

    #[test]
    fn crops_and_rotations_do_not_glide() {
        let a = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        // Moved and scaled evenly (grid arrange, align): glides.
        assert!(same_shape(
            &a,
            &Rect::from_min_size(pos2(50.0, 9.0), vec2(200.0, 150.0))
        ));
        // Turned a quarter or cropped: changes at once.
        assert!(!same_shape(
            &a,
            &Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0))
        ));
        assert!(!same_shape(
            &a,
            &Rect::from_min_size(pos2(0.0, 0.0), vec2(250.0, 300.0))
        ));
    }
}
