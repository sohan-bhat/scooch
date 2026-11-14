#[cfg(feature = "portable")]
mod pick {
    pub fn exp(x: f32) -> f32 {
        libm::expf(x)
    }

    pub fn ln(x: f32) -> f32 {
        libm::logf(x)
    }

    pub fn cbrt(x: f32) -> f32 {
        libm::cbrtf(x)
    }

    pub fn atan2(y: f32, x: f32) -> f32 {
        libm::atan2f(y, x)
    }

    pub fn powf(x: f32, y: f32) -> f32 {
        libm::powf(x, y)
    }

    pub fn pow(x: f64, y: f64) -> f64 {
        libm::pow(x, y)
    }
}

#[cfg(not(feature = "portable"))]
mod pick {
    pub fn exp(x: f32) -> f32 {
        x.exp()
    }

    pub fn ln(x: f32) -> f32 {
        x.ln()
    }

    pub fn cbrt(x: f32) -> f32 {
        x.cbrt()
    }

    pub fn atan2(y: f32, x: f32) -> f32 {
        y.atan2(x)
    }

    pub fn powf(x: f32, y: f32) -> f32 {
        x.powf(y)
    }

    pub fn pow(x: f64, y: f64) -> f64 {
        x.powf(y)
    }
}

pub use pick::*;
