/// Single-range HTTP Range parsing (RFC 9110 s14).
///
/// Seeking in a long track and the offline downloader both depend on this, and a
/// bad byte window shows up as silent corruption rather than an error, so
/// malformed input is rejected explicitly.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedRange {
    Range { start: u64, end: u64 },
    Whole,
    Unsatisfiable,
}

/// All digits, saturating: a number too big for u64 is past the end of any file.
fn digits(value: &str) -> Option<u64> {
    value
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| value.parse().unwrap_or(u64::MAX))
}

pub fn parse_range(header: Option<&str>, size: u64) -> ParsedRange {
    let Some(header) = header else {
        return ParsedRange::Whole;
    };
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return ParsedRange::Whole;
    };

    // Multi-range responses need multipart/byteranges. Nothing here asks for it,
    // and serving the whole file instead is a valid response.
    if spec.is_empty() || spec.contains(',') {
        return ParsedRange::Whole;
    }

    let Some((start, end)) = spec.trim().split_once('-') else {
        return ParsedRange::Whole;
    };
    let (Some(start_value), Some(end_value)) = (
        digits(start)
            .filter(|_| !start.is_empty())
            .or(start.is_empty().then_some(0)),
        digits(end)
            .filter(|_| !end.is_empty())
            .or(end.is_empty().then_some(0)),
    ) else {
        return ParsedRange::Whole;
    };

    // An empty file can satisfy no range at all.
    if size == 0 {
        return ParsedRange::Unsatisfiable;
    }

    if start.is_empty() {
        // Suffix form: "bytes=-500" means the final 500 bytes.
        if end.is_empty() {
            return ParsedRange::Whole;
        }
        if end_value == 0 {
            return ParsedRange::Unsatisfiable;
        }
        return ParsedRange::Range {
            start: size.saturating_sub(end_value),
            end: size - 1,
        };
    }

    if start_value >= size {
        return ParsedRange::Unsatisfiable;
    }

    let last = if end.is_empty() {
        size - 1
    } else {
        end_value.min(size - 1)
    };
    if last < start_value {
        return ParsedRange::Unsatisfiable;
    }

    ParsedRange::Range {
        start: start_value,
        end: last,
    }
}

#[cfg(test)]
mod tests {
    use super::ParsedRange::*;
    use super::*;

    const SIZE: u64 = 1000;

    fn parse(header: &str) -> ParsedRange {
        parse_range(Some(header), SIZE)
    }

    #[test]
    fn serves_the_whole_file_when_no_header_is_sent() {
        assert_eq!(parse_range(None, SIZE), Whole);
    }

    #[test]
    fn parses_an_explicit_window() {
        assert_eq!(
            parse("bytes=100-199"),
            Range {
                start: 100,
                end: 199
            }
        );
    }

    #[test]
    fn treats_an_open_end_as_the_rest_of_the_file() {
        assert_eq!(
            parse("bytes=500-"),
            Range {
                start: 500,
                end: 999
            }
        );
    }

    #[test]
    fn clamps_an_end_past_the_file_to_the_last_byte() {
        assert_eq!(parse("bytes=0-99999"), Range { start: 0, end: 999 });
    }

    #[test]
    fn handles_the_suffix_form() {
        assert_eq!(
            parse("bytes=-200"),
            Range {
                start: 800,
                end: 999
            }
        );
    }

    #[test]
    fn clamps_a_suffix_longer_than_the_file() {
        assert_eq!(parse("bytes=-99999"), Range { start: 0, end: 999 });
    }

    #[test]
    fn rejects_a_start_past_the_end_of_the_file() {
        assert_eq!(parse("bytes=1000-"), Unsatisfiable);
        assert_eq!(parse("bytes=5000-6000"), Unsatisfiable);
    }

    #[test]
    fn rejects_a_backwards_window() {
        assert_eq!(parse("bytes=500-100"), Unsatisfiable);
    }

    #[test]
    fn rejects_a_zero_length_suffix() {
        assert_eq!(parse("bytes=-0"), Unsatisfiable);
    }

    #[test]
    fn ignores_garbage_rather_than_producing_bad_offsets() {
        for header in ["bytes=abc", "bytes=--", "items=0-10", "bytes="] {
            assert_eq!(parse(header), Whole, "{header}");
        }
    }

    #[test]
    fn falls_back_to_the_whole_file_for_multi_range_requests() {
        assert_eq!(parse("bytes=0-10,20-30"), Whole);
    }

    #[test]
    fn cannot_satisfy_any_range_on_an_empty_file() {
        assert_eq!(parse_range(Some("bytes=0-"), 0), Unsatisfiable);
    }

    #[test]
    fn survives_numbers_too_big_for_u64() {
        assert_eq!(parse("bytes=99999999999999999999999-"), Unsatisfiable);
        assert_eq!(
            parse("bytes=0-99999999999999999999999"),
            Range { start: 0, end: 999 }
        );
    }
}
