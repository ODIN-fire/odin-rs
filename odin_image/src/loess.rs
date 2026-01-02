#[inline(always)]
fn cubed (x: f32)->f32 {
    x * x * x
}

pub struct LinearLoess {
    bandwidth: usize,
    weights: Vec<f32>,
}

impl LinearLoess {
    pub fn new (bandwidth: usize)->Self {
        let weights = Self::compute_weights(bandwidth/2);
        LinearLoess{ bandwidth, weights }
    }

    fn compute_weights (b2: usize)->Vec<f32> {
        let mut weights: Vec<f32> = Vec::with_capacity( b2+1);
        for i in 0..=b2 {
            let w = i as f32 / b2 as f32;
            if w < 1.0 {
                weights.push( cubed( 1.0 - cubed(w))); // Cleveland weight function
            } else {
                weights.push( 0.0);
            }
        }
        weights
    }

    #[inline(always)]
    fn weight (&self, d: usize)->f32 {
        if d < self.weights.len() { self.weights[d] } else { 0.0 }
    }

    pub fn smooth (&self, data: &[u32])->Vec<u32> {
        let n = data.len();
        let mut vs = Vec::with_capacity(n);

        let b = self.bandwidth.min(n);
        let b2 = b/2;

        for i in 0..n {
            let mut i0 = if i >= b2 { i - b2 } else { 0 };
            let i1 = (i0 + b).min( n);
            if i1 - i0 < b { i0 = i1.saturating_sub(b); }

            let y = self.weighted_linear( data, i0, i1, i as f32);
            let y: u32 = if y < 0.0 { 0 } else { y as u32 };

            vs.push( y);
        }

        vs
    }

    fn weighted_linear (&self, data: &[u32], i0: usize, i1: usize, x: f32) -> f32 {
        let mut sum_w = 0.0f32;
        let mut sum_wx = 0.0f32;
        let mut sum_wy = 0.0f32;
        let mut sum_wxx = 0.0f32;
        let mut sum_wxy = 0.0f32;

        for i in i0..i1 {
            let xi = i as f32;
            let dist = if xi > x { xi-x } else { x-xi } as usize;
            let w = self.weight( dist);
            let yi = data[i] as f32;
            let wxi = w * xi;

            sum_w += w;
            sum_wx += wxi;
            sum_wy += w * yi;
            sum_wxx += wxi * xi;
            sum_wxy += wxi * yi;
        }

        let denom = sum_w * sum_wxx - sum_wx * sum_wx;
        if denom.abs() < 1e-6 { // this could cause a large numeric error
            self.weighted_mean(data, i0, i1, x as usize)
        } else {
            let s1 = (sum_w * sum_wxy - sum_wx * sum_wy) / denom;
            let s0 = (sum_wy - s1 * sum_wx) / sum_w;
            s0 + s1 * x
        }
    }

    fn weighted_mean( &self, data: &[u32], i0: usize, i1: usize, xi: usize) -> f32 {
        let mut sum_wy = 0.0f32;
        let mut sum_w = 0.0f32;

        for i in i0..i1 {
            let dist = if i > xi { i-xi } else { xi-i };
            let w = self.weight(dist);
            sum_wy += w * data[i] as f32;
            sum_w += w;
        }

        sum_wy / sum_w
    }
}

pub fn linear_loess (data: &[u32], bandwidth: usize)->Vec<u32> {
    let loess = LinearLoess::new( bandwidth);
    loess.smooth(data)
}