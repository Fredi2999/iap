//! Werkzeug: aktuelle Uhrzeit und Datum in UTC und ISO-Format.

use pa_policy::{CapabilityAction, CapabilityRequest, DerivationSource};
use serde_json::json;

use crate::{
    evaluate_and_audit, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput, ToolSpec,
};

pub struct ClockTool;

impl Tool for ClockTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "now".to_owned(),
            description: "Liefert die aktuelle UTC-Zeit als ISO 8601-String.".to_owned(),
            parameters_schema: json!({ "type": "object", "properties": {} }),
            category: "system".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let request = CapabilityRequest {
            action: CapabilityAction::Pure,
            relative_path: None,
            source: invocation.source,
            reason: "now".to_owned(),
        };
        let _ = evaluate_and_audit(&invocation.name, &request, context)?;
        let iso = iso_from_unix_ms(context.now_unix_ms);
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content: iso,
            is_untrusted: matches!(invocation.source, DerivationSource::UntrustedContent),
            truncated_from_bytes: None,
        })
    }
}

/// Formatiert Unix-Millisekunden als ISO 8601-UTC. Handgeschrieben, weil im
/// MVP keine Zeit-Crate zwingend eingezogen werden soll.
fn iso_from_unix_ms(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1000);
    let milli = unix_ms.rem_euclid(1000);
    let (year, month, day, hour, minute, second) = civil_from_days_seconds(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z")
}

fn civil_from_days_seconds(seconds: i64) -> (i32, u8, u8, u8, u8, u8) {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let hour = (time / 3600) as u8;
    let minute = ((time / 60) % 60) as u8;
    let sec = (time % 60) as u8;
    let (year, month, day) = civil_from_days(days);
    (year, month, day, hour, minute, sec)
}

// Nach Howard Hinnant, "date algorithms". Gültig für weite Bereiche.
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u8, d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_formatted_correctly() {
        assert_eq!(iso_from_unix_ms(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn known_date_boundaries_are_stable() {
        // Ende März 2020 (Schaltjahr) muss April folgen.
        let march31 = 1_585_612_800_000;
        assert!(iso_from_unix_ms(march31).starts_with("2020-03-31"));
        assert!(iso_from_unix_ms(march31 + 86_400_000).starts_with("2020-04-01"));
    }
}
