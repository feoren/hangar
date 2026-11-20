//! GPU use on macOS from `ioreg -r -d 1 -w 0 -c IOAccelerator`, as plain text parsing, so it is
//! tested on every platform (evidence: docs/wiki/pages/platforms/gpu-readings.md).
//!
//! Each accelerator's registry entry carries a `PerformanceStatistics` dictionary of numbers on
//! one line. The keys are not documented by Apple and differ between GPU generations, so each
//! value has fallbacks and a missing one stays unknown rather than zero. The registry has no
//! per-process figures readable without root, so macOS gives only the machine.

/// Utilization in percent: Apple silicon and AMD use the first key, older Intel GPUs the second.
const LOAD_KEYS: [&str; 2] = ["Device Utilization %", "GPU Activity(%)"];
/// Memory in use in bytes: Apple silicon's unified memory, then a discrete GPU's VRAM.
const MEMORY_KEYS: [&str; 2] = ["In use system memory", "vramUsedBytes"];

const PREFIX: &str = "\"PerformanceStatistics\" = {";

/// The numbers of every `PerformanceStatistics` dictionary in the output, one map per
/// accelerator. Values that are not plain numbers (nested data) are left out.
fn dictionaries(output: &str) -> Vec<Vec<(&str, f64)>> {
    output
        .lines()
        .filter_map(|line| {
            let body = &line[line.find(PREFIX)? + PREFIX.len()..];
            let body = &body[..body.rfind('}')?];
            Some(
                body.split(',')
                    .filter_map(|pair| {
                        let (key, value) = pair.split_once('=')?;
                        let key = key.trim().strip_prefix('"')?.strip_suffix('"')?;
                        Some((key, value.trim().parse::<f64>().ok()?))
                    })
                    .collect(),
            )
        })
        .collect()
}

fn first_of(dictionary: &[(&str, f64)], keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|wanted| {
        dictionary
            .iter()
            .find(|(key, _)| key == wanted)
            .map(|(_, value)| *value)
    })
}

/// Load and memory of every accelerator: `None` for an output without any
/// `PerformanceStatistics`, otherwise one pair per accelerator for [`super::drm::combine`].
pub fn parse(output: &str) -> Option<Vec<(Option<f64>, Option<u64>)>> {
    let dictionaries = dictionaries(output);
    if dictionaries.is_empty() {
        return None;
    }
    Some(
        dictionaries
            .iter()
            .map(|dictionary| {
                let load = first_of(dictionary, &LOAD_KEYS);
                let memory = first_of(dictionary, &MEMORY_KEYS)
                    .filter(|bytes| bytes.is_finite() && *bytes >= 0.0)
                    .map(|bytes| bytes.round() as u64);
                (load, memory)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One accelerator of an M3 Pro, the line shortened to the keys that matter, the rest of the
    /// entry as `ioreg` prints it.
    const APPLE_SILICON: &str = "+-o AGXAcceleratorG15X  <class AGXAcceleratorG15X, id 0x1000004ae, registered>\n\
        {\n\
          \"IOClass\" = \"AGXAcceleratorG15X\"\n\
          \"PerformanceStatistics\" = {\"In use system memory (driver)\"=0,\"Alloc system memory\"=16749051904,\"Tiler Utilization %\"=7,\"Device Utilization %\"=16,\"Renderer Utilization %\"=15,\"In use system memory\"=568164352,\"recoveryCount\"=0}\n\
          \"CFBundleIdentifier\" = \"com.apple.AGXG15X\"\n\
        }\n";

    #[test]
    fn apple_silicon_gives_load_and_memory() {
        assert_eq!(
            parse(APPLE_SILICON),
            Some(vec![(Some(16.0), Some(568_164_352))])
        );
    }

    #[test]
    fn the_driver_share_is_not_the_memory_in_use() {
        // "In use system memory (driver)" comes first and must not match the plain key.
        let reading = parse(APPLE_SILICON).unwrap();
        assert_ne!(reading[0].1, Some(0));
    }

    #[test]
    fn older_keys_are_read_too() {
        let output =
            "  \"PerformanceStatistics\" = {\"GPU Activity(%)\"=3,\"vramUsedBytes\"=1048576}\n\
            \"PerformanceStatistics\" = {\"Device Utilization %\"=40}\n";
        assert_eq!(
            parse(output),
            Some(vec![(Some(3.0), Some(1_048_576)), (Some(40.0), None)])
        );
    }

    #[test]
    fn missing_or_odd_values_stay_unknown() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("\"IOClass\" = \"IOAccelerator\"\n"), None);
        let odd = "\"PerformanceStatistics\" = {\"Device Utilization %\"=busy,\"x\"=<01>}\n";
        assert_eq!(parse(odd), Some(vec![(None, None)]));
        let negative = "\"PerformanceStatistics\" = {\"In use system memory\"=-5}\n";
        assert_eq!(parse(negative), Some(vec![(None, None)]));
        let open = "\"PerformanceStatistics\" = {\"Device Utilization %\"=5\n";
        assert_eq!(parse(open), None, "no closing brace");
    }
}
