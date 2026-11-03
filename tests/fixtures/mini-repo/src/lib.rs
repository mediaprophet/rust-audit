//! Module doc comment for the fixture.

// A plain line comment.

/// Integrate velocity using rk4.
/// Second doc line.
pub fn integrate_rk4(velocity: f32, dt: f32) -> f32 {
    // inner comment
    velocity * dt
}

pub struct Body {
    pub mass: f32,
}

impl Body {
    /// Apply a force to the rigidbody.
    pub fn apply_force(&mut self, force: f32) {
        self.mass += force;
        self.clamp();
        integrate_rk4(self.mass, 0.1);
    }

    fn clamp(&self) {}
}
