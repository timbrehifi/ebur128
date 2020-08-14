// Copyright (c) 2011 Jan Kokemüller
// Copyright (c) 2020 Sebastian Dröge <sebastian@centricular.com>
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use crate::interp::Interp;

#[derive(Debug)]
pub struct TruePeak {
    interp: Interp,
    rate: u32,
    channels: u32,
    buffer_output: Vec<f64>,
}

impl TruePeak {
    pub fn new(rate: u32, channels: u32) -> Option<Self> {
        let samples_in_100ms = (rate + 5) / 10;

        let (interp, interp_factor) = if rate < 96_000 {
            (Interp::new(49, 4, channels), 4)
        } else if rate < 192_000 {
            (Interp::new(49, 2, channels), 2)
        } else {
            return None;
        };

        let buffer_input = vec![0.0; 4 * samples_in_100ms as usize * channels as usize];
        let buffer_output = vec![0.0; buffer_input.len() * interp_factor];

        Some(Self {
            interp,
            rate,
            channels,
            buffer_output,
        })
    }

    pub fn process(&mut self, src: &[f64], src_index: usize, frames: usize, peaks: &mut [f64]) {
        let src_stride = src.len() / self.channels as usize;

        assert!(src_index + frames <= src_stride);
        assert!(src_stride * self.interp.get_factor() <= self.buffer_output.len());
        assert!(peaks.len() == self.channels as usize);

        if frames == 0 {
            return;
        }

        let interp_factor = self.interp.get_factor();

        dbg!(&src);

        self.interp.process(
            src,
            src_index,
            frames,
            &mut self.buffer_output[..(frames * self.channels as usize * interp_factor)],
        );

        dbg!(&self.buffer_output[..(frames * self.channels as usize * interp_factor)]);

        // Find the maximum
        for (o, peak) in self.buffer_output[..(frames * self.channels as usize * interp_factor)]
            .chunks_exact(frames * interp_factor)
            .zip(peaks)
        {
            for v in o {
                if *v > *peak {
                    *peak = *v;
                }
            }
        }
    }
}

#[cfg(feature = "c-tests")]
use std::os::raw::c_void;

#[cfg(feature = "c-tests")]
extern "C" {
    pub fn true_peak_create_c(rate: u32, channels: u32) -> *mut c_void;
    pub fn true_peak_check_double_c(
        tp: *mut c_void,
        frames: usize,
        src: *const f64,
        peaks: *mut f64,
    );
    pub fn true_peak_destroy_c(tp: *mut c_void);
}

#[cfg(feature = "c-tests")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Signal;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn compare_c_impl(signal: Signal<f64>) -> quickcheck::TestResult {
        use float_cmp::approx_eq;

        if signal.rate >= 192_000 {
            return quickcheck::TestResult::discard();
        }

        // Maximum of 400ms but our input is up to 5000ms, so distribute it evenly
        // by shrinking accordingly.
        let frames = signal.data.len() / signal.channels as usize;
        let frames = std::cmp::min(2 * frames / 25, 4 * ((signal.rate as usize + 5) / 10));

        if frames == 0 {
            return quickcheck::TestResult::discard();
        }

        let mut peaks = vec![0.0f64; signal.channels as usize];
        let mut peaks_c = vec![0.0f64; signal.channels as usize];

        {
            // Need to deinterleave the input
            let mut data_in_tmp = vec![0.0f64; frames * signal.channels as usize];

            for (c, out) in data_in_tmp.chunks_exact_mut(frames).enumerate() {
                for (s, out) in out.iter_mut().enumerate() {
                    *out = signal.data[signal.channels as usize * s + c] as f64;
                }
            }

            let mut tp = TruePeak::new(signal.rate, signal.channels).unwrap();
            tp.process(&data_in_tmp, 0, frames, &mut peaks);
        }

        unsafe {
            let tp = true_peak_create_c(signal.rate, signal.channels);
            assert!(!tp.is_null());
            true_peak_check_double_c(tp, frames, signal.data.as_ptr(), peaks_c.as_mut_ptr());
            true_peak_destroy_c(tp);
        }

        dbg!(&peaks);
        dbg!(&peaks_c);
        for (i, (r, c)) in peaks.iter().zip(peaks_c.iter()).enumerate() {
            assert!(
                approx_eq!(f64, *r, *c, ulps = 2),
                "Rust and C implementation differ at channel {}: {} != {}",
                i,
                r,
                c
            );
        }

        quickcheck::TestResult::passed()
    }
}
