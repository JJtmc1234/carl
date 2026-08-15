//! The graphics card and the thermometers, where this machine will tell us about them.
//!
//! Both sources here were checked on JJ's machine before being written down.
//!
//! **GPU** comes from `nvidia-smi`, which is at `/usr/bin/nvidia-smi` and answers as an
//! ordinary user. On this machine it reports an NVIDIA GeForce RTX 5090 Laptop GPU with 24463
//! MiB of memory. It costs a process per sample, which is why it is sampled no faster than
//! anything else and why a failure to run is reported as unknown rather than retried in a loop.
//!
//! **Temperatures** come from `/sys/class/thermal/thermal_zone*/temp`, which is world readable
//! here and gives millidegrees. The zone named `x86_pkg_temp` is the CPU package, which is the
//! one worth showing. The rest are board sensors that mean little without a datasheet.
//!
//! Neither is required. A machine without an NVIDIA card, or with the thermal zones locked
//! down, reports unknown and the rest of the panel carries on.

use std::process::Command;

/// The thermal zone whose reading is the CPU package, in preference order.
const CPU_ZONE_TYPES: &[&str] = &["x86_pkg_temp", "TCPU", "acpitz"];

/// What the graphics card says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub utilisation_percent: Option<f64>,
    pub memory_used_mib: Option<u64>,
    pub memory_total_mib: Option<u64>,
    pub temperature_c: Option<f64>,
}

/// Parses one comma separated line of `nvidia-smi --format=csv,noheader`.
///
/// Every field after the name is optional on purpose. `nvidia-smi` prints `[N/A]` for anything
/// the card does not expose, and a laptop card that has powered down its memory controller will
/// do exactly that. Those become `None`, never zero.
pub fn parse_query(line: &str) -> Option<Gpu> {
    let mut fields = line.split(',').map(str::trim);
    let name = fields.next()?.to_string();
    if name.is_empty() {
        return None;
    }

    // Each value arrives with its unit attached, for example `0 %` or `16 MiB`.
    let number = |f: Option<&str>| -> Option<f64> {
        let raw = f?;
        if raw.contains("N/A") {
            return None;
        }
        raw.split_whitespace().next()?.parse().ok()
    };

    let utilisation_percent = number(fields.next());
    let memory_used_mib = number(fields.next()).map(|v| v as u64);
    let memory_total_mib = number(fields.next()).map(|v| v as u64);
    let temperature_c = number(fields.next());

    Some(Gpu {
        name,
        utilisation_percent,
        memory_used_mib,
        memory_total_mib,
        temperature_c,
    })
}

/// Asks `nvidia-smi` for the first card.
///
/// `None` when the binary is absent, fails, or prints something unrecognisable, which covers
/// every machine that is not this one.
pub fn read_gpu() -> Option<Gpu> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_query(String::from_utf8_lossy(&out.stdout).lines().next()?)
}

/// Turns a `thermal_zone` reading in millidegrees into degrees.
pub fn parse_millidegrees(text: &str) -> Option<f64> {
    let raw: i64 = text.trim().parse().ok()?;
    // Zones that are present but not reporting sit at or below absolute zero once converted.
    (raw > -273_000).then_some(raw as f64 / 1000.0)
}

/// The CPU package temperature, from the first thermal zone that names itself usefully.
pub fn read_cpu_temperature() -> Option<f64> {
    let zones = std::fs::read_dir("/sys/class/thermal").ok()?;
    let mut found: Vec<(String, f64)> = Vec::new();

    for zone in zones.flatten() {
        let path = zone.path();
        if !path
            .file_name()?
            .to_string_lossy()
            .starts_with("thermal_zone")
        {
            continue;
        }
        let Ok(kind) = std::fs::read_to_string(path.join("type")) else {
            continue;
        };
        let Ok(temp) = std::fs::read_to_string(path.join("temp")) else {
            continue;
        };
        if let Some(c) = parse_millidegrees(&temp) {
            found.push((kind.trim().to_string(), c));
        }
    }

    for wanted in CPU_ZONE_TYPES {
        if let Some((_, c)) = found.iter().find(|(k, _)| k == wanted) {
            return Some(*c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact line this machine produced when the source was verified.
    const REAL: &str = "NVIDIA GeForce RTX 5090 Laptop GPU, 0 %, 16 MiB, 24463 MiB, 40";

    #[test]
    fn the_real_line_from_this_machine_parses() {
        let g = parse_query(REAL).unwrap();
        assert_eq!(g.name, "NVIDIA GeForce RTX 5090 Laptop GPU");
        assert_eq!(g.utilisation_percent, Some(0.0));
        assert_eq!(g.memory_used_mib, Some(16));
        assert_eq!(g.memory_total_mib, Some(24463));
        assert_eq!(g.temperature_c, Some(40.0));
    }

    /// A card that does not expose a field says so, and that must not become zero.
    #[test]
    fn a_field_the_card_does_not_expose_is_unknown_rather_than_zero() {
        let g = parse_query("Some GPU, [N/A], [N/A], 8192 MiB, [N/A]").unwrap();
        assert_eq!(g.utilisation_percent, None);
        assert_eq!(g.memory_used_mib, None);
        assert_eq!(g.memory_total_mib, Some(8192));
        assert_eq!(g.temperature_c, None);
    }

    #[test]
    fn a_short_or_empty_line_is_refused() {
        assert_eq!(parse_query(""), None);
        let only_name = parse_query("Some GPU").unwrap();
        assert_eq!(
            only_name.utilisation_percent, None,
            "missing fields are not zero"
        );
    }

    #[test]
    fn millidegrees_become_degrees() {
        assert_eq!(parse_millidegrees("45000\n"), Some(45.0));
        assert_eq!(parse_millidegrees("38050"), Some(38.05));
        assert_eq!(parse_millidegrees("not a number"), None);
        assert_eq!(
            parse_millidegrees("-274000"),
            None,
            "a zone that is not reporting"
        );
    }

    /// Against the real machine. Both sources were verified present here, so a failure means
    /// something changed rather than that the test is optimistic.
    #[test]
    fn this_machine_reports_a_gpu_and_a_cpu_temperature() {
        let gpu = read_gpu().expect("nvidia-smi answered when this was written");
        assert!(!gpu.name.is_empty());
        assert!(
            gpu.memory_total_mib.unwrap_or(0) > 1024,
            "a card with under a gibibyte is not this one"
        );

        let temp = read_cpu_temperature().expect("thermal zones were readable when written");
        assert!(
            (10.0..115.0).contains(&temp),
            "a CPU at {temp}C is either off or on fire"
        );
    }
}
