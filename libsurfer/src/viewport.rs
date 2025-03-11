use std::ops::RangeInclusive;

use derive_more::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};
use num::{BigInt, BigRational, FromPrimitive, ToPrimitive};
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Add,
    Sub,
    Mul,
    Neg,
    AddAssign,
    SubAssign,
    PartialOrd,
    PartialEq,
)]
pub struct Relative(pub f64);

impl Relative {
    pub fn absolute(&self, num_timestamps: &BigInt) -> Absolute {
        Absolute(
            self.0
                * num_timestamps
                    .to_f64()
                    .expect("Failed to convert timestamp to f64"),
        )
    }

    pub fn min(&self, other: &Relative) -> Self {
        Self(self.0.min(other.0))
    }

    pub fn max(&self, other: &Relative) -> Self {
        Self(self.0.max(other.0))
    }
}

impl std::ops::Div for Relative {
    type Output = Relative;

    fn div(self, rhs: Self) -> Self::Output {
        Self(self.0 / rhs.0)
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Add, Sub, Mul, Neg, Div, PartialOrd, PartialEq,
)]
pub struct Absolute(pub f64);

impl Absolute {
    pub fn relative(&self, num_timestamps: &BigInt) -> Relative {
        Relative(
            self.0
                / num_timestamps
                    .to_f64()
                    .expect("Failed to convert timestamp to f64"),
        )
    }
}

impl std::ops::Div for Absolute {
    type Output = Absolute;

    fn div(self, rhs: Self) -> Self::Output {
        Self(self.0 / rhs.0)
    }
}

impl From<&BigInt> for Absolute {
    fn from(value: &BigInt) -> Self {
        Self(value.to_f64().expect("Failed to convert timestamp to f64"))
    }
}

fn default_edge_space() -> f64 {
    0.2
}

fn default_min_width() -> Absolute {
    Absolute(0.5)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub curr_left: Relative,
    pub curr_right: Relative,

    target_left: Relative,
    target_right: Relative,

    move_start_left: Relative,
    move_start_right: Relative,

    // Number of seconds since the the last time a movement happened
    move_duration: Option<f32>,
    pub move_strategy: ViewportStrategy,
    #[serde(skip, default = "default_edge_space")]
    edge_space: f64,

    #[serde(skip, default = "default_min_width")]
    min_width: Absolute,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            curr_left: Relative(0.0),
            curr_right: Relative(1.0),
            target_left: Relative(0.0),
            target_right: Relative(1.0),
            move_start_left: Relative(0.0),
            move_start_right: Relative(1.0),
            move_duration: None,
            move_strategy: ViewportStrategy::Instant,
            edge_space: default_edge_space(),
            min_width: default_min_width(),
        }
    }
}

impl Viewport {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn left_edge_time(self, num_timestamps: &BigInt) -> BigInt {
        BigInt::from(self.curr_left.absolute(num_timestamps).0 as i64)
    }
    pub fn right_edge_time(self, num_timestamps: &BigInt) -> BigInt {
        BigInt::from(self.curr_right.absolute(num_timestamps).0 as i64)
    }

    pub fn as_absolute_time(&self, x: f64, view_width: f32, num_timestamps: &BigInt) -> Absolute {
        let time_spacing = self.width_absolute(num_timestamps) / view_width as f64;

        self.curr_left.absolute(num_timestamps) + time_spacing * x
    }

    pub fn as_time_bigint(&self, x: f32, view_width: f32, num_timestamps: &BigInt) -> BigInt {
        let Viewport {
            curr_left: left,
            curr_right: right,
            ..
        } = &self;

        let big_right = BigRational::from_f64(right.absolute(num_timestamps).0)
            .unwrap_or_else(|| BigRational::from_u8(1).unwrap());
        let big_left = BigRational::from_f64(left.absolute(num_timestamps).0)
            .unwrap_or_else(|| BigRational::from_u8(1).unwrap());
        let big_width =
            BigRational::from_f32(view_width).unwrap_or_else(|| BigRational::from_u8(1).unwrap());
        let big_x = BigRational::from_f32(x).unwrap_or_else(|| BigRational::from_u8(1).unwrap());

        let time = big_left.clone() + (big_right - big_left) / big_width * big_x;
        time.round().to_integer()
    }

    pub fn to_time_f64(&self, x: f64, view_width: f32, num_timestamps: &BigInt) -> Absolute {
        let time_spacing = self.width_absolute(num_timestamps) / view_width as f64;

        self.curr_left.absolute(num_timestamps) + time_spacing * x
    }

    pub fn to_time_bigint(&self, x: f32, view_width: f32, num_timestamps: &BigInt) -> BigInt {
        let Viewport {
            curr_left: left,
            curr_right: right,
            ..
        } = &self;

        let big_right = BigRational::from_f64(right.absolute(num_timestamps).0)
            .unwrap_or_else(|| BigRational::from_u8(1).unwrap());
        let big_left = BigRational::from_f64(left.absolute(num_timestamps).0)
            .unwrap_or_else(|| BigRational::from_u8(1).unwrap());
        let big_width =
            BigRational::from_f32(view_width).unwrap_or_else(|| BigRational::from_u8(1).unwrap());
        let big_x = BigRational::from_f32(x).unwrap_or_else(|| BigRational::from_u8(1).unwrap());

        let time = big_left.clone() + (big_right - big_left) / big_width * big_x;
        time.round().to_integer()
    }

    /// Computes which x-pixel corresponds to the specified time adduming the viewport is rendered
    /// into a viewport of `view_width`
    pub fn pixel_from_time(&self, time: &BigInt, view_width: f32, num_timestamps: &BigInt) -> f32 {
        let distance_from_left =
            Absolute(time.to_f64().unwrap()) - self.curr_left.absolute(num_timestamps);

        (((distance_from_left / self.width_absolute(num_timestamps)).0) * (view_width as f64))
            as f32
    }

    pub fn pixel_from_time_f64(
        &self,
        time: Absolute,
        view_width: f32,
        num_timestamps: &BigInt,
    ) -> f32 {
        let distance_from_left = time - self.curr_left.absolute(num_timestamps);

        (((distance_from_left / self.width_absolute(num_timestamps)).0) * (view_width as f64))
            as f32
    }

    pub fn pixel_from_absolute_time(
        &self,
        time: Absolute,
        view_width: f32,
        num_timestamps: &BigInt,
    ) -> f32 {
        let distance_from_left = time - self.curr_left.absolute(num_timestamps);

        (((distance_from_left / self.width_absolute(num_timestamps)).0) * (view_width as f64))
            as f32
    }

    /// Return new viewport for a different file length
    ///
    /// Tries to keep the current zoom level and position. If zoom is not possible it
    /// will zoom in as much as needed to keep border margins. If the new waveform is
    /// too short, the viewport will be moved to the left as much as needed for the zoom level.
    pub fn clip_to(&self, old_num_timestamps: &BigInt, new_num_timestamps: &BigInt) -> Viewport {
        let left_timestamp = self.curr_left.absolute(old_num_timestamps);
        let right_timestamp = self.curr_right.absolute(old_num_timestamps);
        let absolute_width = right_timestamp - left_timestamp;

        let new_absolute_width = new_num_timestamps
            .to_f64()
            .expect("Failed to convert timestamp to f64")
            * (2.0 * self.edge_space);
        let (left, right) = if absolute_width.0 > new_absolute_width {
            // is the new waveform so short that we can't keep the current zoom level?
            (Relative(-self.edge_space), Relative(1.0 + self.edge_space))
        } else {
            // our zoom level is achievable but we don't know the waveform is long enough

            let unmoved_right = Relative(
                (left_timestamp + absolute_width).0.to_f64().unwrap()
                    / new_num_timestamps.to_f64().unwrap(),
            );
            if unmoved_right <= Relative(1.0 + self.edge_space) {
                // waveform is long enough, keep current view as-is
                (self.curr_left, unmoved_right)
            } else {
                // waveform is too short, clip end to the right edge (including empty space)
                // since we checked above for zoom level, we know that there must be enough
                // waveform to the left to keep the current zoom level
                (
                    Relative(1.0 + self.edge_space - absolute_width.0),
                    Relative(1.0 + self.edge_space),
                )
            }
        };

        Viewport {
            curr_left: left,
            curr_right: right,
            target_left: left,
            target_right: right,
            move_start_left: left,
            move_start_right: right,
            move_duration: None,
            move_strategy: self.move_strategy,
            edge_space: self.edge_space,
            min_width: self.min_width,
        }
    }

    #[inline]
    fn width(&self) -> Relative {
        self.curr_right - self.curr_left
    }

    #[inline]
    fn width_absolute(&self, num_timestamps: &BigInt) -> Absolute {
        self.width().absolute(num_timestamps)
    }

    pub fn go_to_time(&mut self, center: &BigInt, num_timestamps: &BigInt) {
        let center_point: Absolute = center.into();
        let half_width = self.half_width_absolute(num_timestamps);

        let target_left = (center_point - half_width).relative(num_timestamps);
        let target_right = (center_point + half_width).relative(num_timestamps);
        self.set_viewport_to_clipped(target_left, target_right, num_timestamps);
    }

    pub fn zoom_to_fit(&mut self) {
        self.set_target_left(Relative(0.0));
        self.set_target_right(Relative(1.0));
    }

    pub fn go_to_start(&mut self) {
        let old_width = self.width();
        self.set_target_left(Relative(0.0));
        self.set_target_right(old_width);
    }

    pub fn go_to_end(&mut self) {
        self.set_target_left(Relative(1.0) - self.width());
        self.set_target_right(Relative(1.0));
    }

    pub fn handle_canvas_zoom(
        &mut self,
        mouse_ptr_timestamp: Option<BigInt>,
        delta: f64,
        num_timestamps: &BigInt,
    ) {
        // Zoom or scroll
        let Viewport {
            curr_left: left,
            curr_right: right,
            ..
        } = &self;

        let (target_left, target_right) =
            match mouse_ptr_timestamp.map(|t| Absolute::from(&t).relative(num_timestamps)) {
                Some(mouse_location) => (
                    (*left - mouse_location) / Relative(delta) + mouse_location,
                    (*right - mouse_location) / Relative(delta) + mouse_location,
                ),
                None => {
                    let mid_point = self.midpoint();
                    let offset = self.half_width() * delta;

                    (mid_point - offset, mid_point + offset)
                }
            };

        self.set_viewport_to_clipped(target_left, target_right, num_timestamps);
    }

    pub fn handle_canvas_scroll(&mut self, deltay: f64) {
        // Scroll 5% of the viewport per scroll event.
        // One scroll event yields 50
        let scroll_step = -self.width() / Relative(50. * 20.);
        let scaled_deltay = scroll_step * deltay;
        self.set_viewport_to_clipped_no_width_check(
            self.curr_left + scaled_deltay,
            self.curr_right + scaled_deltay,
        );
    }

    fn set_viewport_to_clipped(
        &mut self,
        target_left: Relative,
        target_right: Relative,
        num_timestamps: &BigInt,
    ) {
        let rel_min_width = self.min_width.relative(num_timestamps);

        if (target_right - target_left) <= rel_min_width + Relative(f64::EPSILON) {
            let center = (target_left + target_right) * 0.5;
            self.set_viewport_to_clipped_no_width_check(
                center - rel_min_width,
                center + rel_min_width,
            );
        } else {
            self.set_viewport_to_clipped_no_width_check(target_left, target_right);
        }
    }

    fn set_viewport_to_clipped_no_width_check(
        &mut self,
        target_left: Relative,
        target_right: Relative,
    ) {
        let width = target_right - target_left;

        let abs_min = Relative(-self.edge_space);
        let abs_max = Relative(1.0 + self.edge_space);

        let max_right = Relative(1.0) + width * self.edge_space;
        let min_left = -width * self.edge_space;
        if width > (abs_max - abs_min) {
            self.set_target_left(abs_min);
            self.set_target_right(abs_max);
        } else if target_left < min_left {
            self.set_target_left(min_left);
            self.set_target_right(min_left + width);
        } else if target_right > max_right {
            self.set_target_left(max_right - width);
            self.set_target_right(max_right);
        } else {
            self.set_target_left(target_left);
            self.set_target_right(target_right);
        }
    }

    #[inline]
    fn midpoint(&self) -> Relative {
        (self.curr_right + self.curr_left) * 0.5
    }

    #[inline]
    fn half_width(&self) -> Relative {
        self.width() * 0.5
    }

    #[inline]
    fn half_width_absolute(&self, num_timestamps: &BigInt) -> Absolute {
        (self.width() * 0.5).absolute(num_timestamps)
    }

    pub fn zoom_to_range(&mut self, left: &BigInt, right: &BigInt, num_timestamps: &BigInt) {
        self.set_viewport_to_clipped(
            Absolute::from(left).relative(num_timestamps),
            Absolute::from(right).relative(num_timestamps),
            num_timestamps,
        );
    }

    pub fn go_to_cursor_if_not_in_view(
        &mut self,
        cursor: &BigInt,
        num_timestamps: &BigInt,
    ) -> bool {
        let fcursor = cursor.into();
        if fcursor <= self.curr_left.absolute(num_timestamps)
            || fcursor >= self.curr_right.absolute(num_timestamps)
        {
            self.go_to_time_f64(fcursor, num_timestamps);
            true
        } else {
            false
        }
    }

    pub fn go_to_time_f64(&mut self, center: Absolute, num_timestamps: &BigInt) {
        let half_width = (self.curr_right.absolute(num_timestamps)
            - self.curr_left.absolute(num_timestamps))
            / 2.;

        self.set_viewport_to_clipped(
            (center - half_width).relative(num_timestamps),
            (center + half_width).relative(num_timestamps),
            num_timestamps,
        );
    }

    fn set_target_left(&mut self, target_left: Relative) {
        if let ViewportStrategy::Instant = self.move_strategy {
            self.curr_left = target_left
        } else {
            self.target_left = target_left;
            self.move_start_left = self.curr_left;
            self.move_duration = Some(0.);
        }
    }
    fn set_target_right(&mut self, target_right: Relative) {
        if let ViewportStrategy::Instant = self.move_strategy {
            self.curr_right = target_right
        } else {
            self.target_right = target_right;
            self.move_start_right = self.curr_right;
            self.move_duration = Some(0.);
        }
    }

    pub fn move_viewport(&mut self, frame_time: f32) {
        match &self.move_strategy {
            ViewportStrategy::Instant => {
                self.curr_left = self.target_left;
                self.curr_right = self.target_right;
                self.move_duration = None;
            }
            ViewportStrategy::EaseInOut { duration } => {
                if let Some(move_duration) = &mut self.move_duration {
                    if *move_duration + frame_time >= *duration {
                        self.move_duration = None;
                        self.curr_left = self.target_left;
                        self.curr_right = self.target_right;
                    } else {
                        *move_duration += frame_time;

                        self.curr_left = Relative(ease_in_out_size(
                            self.move_start_left.0..=self.target_left.0,
                            (*move_duration as f64) / (*duration as f64),
                        ));
                        self.curr_right = Relative(ease_in_out_size(
                            self.move_start_right.0..=self.target_right.0,
                            (*move_duration as f64) / (*duration as f64),
                        ));
                    }
                }
            }
        }
    }

    pub fn is_moving(&self) -> bool {
        self.move_duration.is_some()
    }
}

pub fn ease_in_out_size(r: RangeInclusive<f64>, t: f64) -> f64 {
    r.start() + ((r.end() - r.start()) * -((std::f64::consts::PI * t).cos() - 1.) / 2.)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ViewportStrategy {
    Instant,
    EaseInOut { duration: f32 },
}
