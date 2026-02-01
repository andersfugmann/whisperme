//! FFT spectrum analysis for frequency band dB levels.

use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};

pub const FFT_SIZE: usize = 256;
pub const FREQ_BANDS: [(f32, f32); 5] = [
    (62.0, 250.0),
    (250.0, 500.0),
    (500.0, 2000.0),
    (2000.0, 4000.0),
    (4000.0, 8000.0),
];

/// Compute dB levels for each frequency band from audio samples.
/// Returns None if FFT fails, otherwise returns dB values per band.
pub fn band_db_levels(samples: &[f32; FFT_SIZE], sample_rate: u32) -> Option<[f32; 5]> {
    let spectrum = samples_fft_to_spectrum(
        &hann_window(samples),
        sample_rate,
        FrequencyLimit::Range(62.0.into(), 8000.0.into()),
        Some(&divide_by_N_sqrt),
    )
    .ok()?;

    Some(std::array::from_fn(|i| {
        let (low, high) = FREQ_BANDS[i];
        let (sum, count) = spectrum.data().iter().fold((0.0f32, 0u32), |(s, c), (f, v)| {
            let hz = f.val();
            if hz >= low && hz < high { (s + v.val(), c + 1) } else { (s, c) }
        });
        match count {
            0 => -80.0,
            _ => 20.0 * (sum / count as f32).log10(),
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 16000;

    #[test]
    fn test_freq_bands_coverage() {
        for window in FREQ_BANDS.windows(2) {
            assert!(
                window[0].1 <= window[1].0,
                "Bands should not overlap: {:?} and {:?}",
                window[0],
                window[1]
            );
        }

        assert!(
            FREQ_BANDS[FREQ_BANDS.len() - 1].1 <= (SAMPLE_RATE / 2) as f32,
            "Last band should not exceed Nyquist frequency"
        );
    }
}
