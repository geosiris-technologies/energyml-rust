# 0045 - Text and numbers written by hand have one copy each

Status: accepted

## Context

A review of the workspace found the same small helpers written again in crate after crate, and the copies had
drifted. XML escaping existed in nine places: the SVG writers of `energyml-section` and `energyml-sheet` dropped the
control characters XML 1.0 cannot carry; the VTK, Galaxy, `core.xml`, `[Content_Types].xml` and `AnyXml` writers
did not, and wrote a document no parser reads. Two of them did not escape `>`. The XML unescape was written twice,
one copy cutting an error message at a byte offset that is not always a character boundary. Base 64 existed twice,
the civil-date arithmetic three times, the short number formatter in six views (one of them mapping `-0` to `0`,
five not, and the text goes into element keys such as `#md=...`), the plain number formatter twice, and a JSON
string escaper in two crates. One copy, `format_number` of the sheet reader, printed 1e20 as `i64::MAX`.

## Decision

One definition each, in the lowest crate that serves every caller:

| What | Where |
|---|---|
| XML text, attribute, unescape | `energyml_core::xml::{escape_text, escape_attr, unescape}` |
| JSON string | `energyml_core::json::quote` (written by `serde_json`) |
| Base 64 | `energyml_core::base64::{encode, decode}` |
| Civil calendar | `energyml_core::calendar::{days_from_civil, civil_from_days}` |
| A number without useless zeros | `energyml_core::text::plain_number` |
| A number written short, `n` decimals | `energyml_sheet::text::short` |

The rules the doors settle, where the copies disagreed:

- **Text escapes `&`, `<`, `>`; an attribute value also `"` and `'`** (XML 1.0, 2.4 and 3.1). The SVG writers go
  through `escape_attr` for both, so their output does not change.
- **A character XML 1.0 cannot carry is dropped**, in text and in attributes (production `Char`, 2.2): a title from a
  file may hold anything, and a missing control character is better than an ill-formed document.
- **`unescape` refuses by name** an `&` with no `;`, an entity XML 1.0 does not predefine, and a reference to a
  character it cannot carry (`&#0;`).
- **`short` never writes `-0`**, and writes a non-finite value as Rust does; the SVG number `num` (a coordinate that
  is not a number is not an SVG) writes it `0`.
- **`plain_number` writes an integer only below 1e15**: beyond it the cast saturates.

The old names stay where they were published (`energyml_domain::attachments::{encode_base64, decode_base64}`,
`energyml_section::svg::escape`, `energyml_sheet::draw::svg::escape`) and call the door.

## Consequences

- A fix to escaping, base 64 or the calendar is made once.
- `energyml-core` gains no dependency: base 64 and the calendar are small and written out; `quote` uses the
  `serde_json` it already has behind the `json` feature.
- Not unified, on purpose: the rules crate keeps its own JSON escaper (it must stay dependency-free); the JSON
  number policy for `NaN` differs between the MCP tools (`"NaN"`) and the series reader (`null`), each a contract of
  its consumer; `energyml_domain::raw_xml::decode` is a lenient reader of the five entities for a byte-level scanner.
