//! CPU temperature from the ACPI thermal zones Windows publishes, for when
//! the sensor service has none. Many desktops publish placeholder zones,
//! so readings that cannot be a running CPU are left out.

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

/// Tenths of a kelvin.
const COUNTER: &str = r"\Thermal Zone Information(*)\High Precision Temperature";
const KELVIN_OFFSET: f64 = 273.15;
/// 301.0 K, the value firmware commonly reports for a zone with no sensor.
const PLACEHOLDER_TENTHS: f64 = 3010.0;
const PLAUSIBLE_C: std::ops::RangeInclusive<f64> = 20.0..=125.0;

/// The hottest zone that could be a CPU, in °C.
pub fn cpu_temp(tenths_kelvin: &[(String, f64)]) -> Option<f64> {
    tenths_kelvin
        .iter()
        .filter(|(_, t)| *t != PLACEHOLDER_TENTHS)
        .map(|(_, t)| t / 10.0 - KELVIN_OFFSET)
        .filter(|c| PLAUSIBLE_C.contains(c))
        .reduce(f64::max)
}

/// Fills CPU temperature from the thermal zones when nothing earlier did.
#[derive(Default)]
pub struct ThermalZoneSource {
    /// Built on first need; `None` inside once it could not be.
    counter: Option<Option<(Query, Counter)>>,
}

impl Source for ThermalZoneSource {
    fn id(&self) -> SourceId {
        SourceId::ThermalZones
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        if out.get(MetricKey::CpuTempC).is_some() {
            return Ok(());
        }
        let counter = self.counter.get_or_insert_with(|| {
            let query = Query::new().ok()?;
            let counter = query.add(COUNTER).ok()?;
            Some((query, counter))
        });
        let Some((query, counter)) = counter else {
            return Ok(());
        };
        if query.collect().is_err() {
            return Ok(());
        }
        if let Some(c) = counter.array().ok().as_deref().and_then(cpu_temp) {
            out.set(MetricKey::CpuTempC, c);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones(tenths: &[f64]) -> Vec<(String, f64)> {
        tenths
            .iter()
            .enumerate()
            .map(|(i, &t)| (format!(r"\_TZ.TZ{i:02}"), t))
            .collect()
    }

    #[test]
    fn placeholder_and_implausible_zones_are_left_out() {
        assert_eq!(cpu_temp(&zones(&[3010.0, 2900.0])), None);
        assert_eq!(cpu_temp(&zones(&[])), None);
        assert_eq!(cpu_temp(&zones(&[4500.0])), None, "177 °C");
    }

    #[test]
    fn the_hottest_real_zone_wins() {
        let c = cpu_temp(&zones(&[3010.0, 3281.5, 3431.5])).unwrap();
        assert!((c - 70.0).abs() < 1e-9);
    }

    #[test]
    fn sampling_never_panics() {
        let mut out = Snapshot::default();
        ThermalZoneSource::default().sample(&mut out).unwrap();
    }
}
