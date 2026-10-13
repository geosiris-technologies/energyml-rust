//! The table reader, against the specification.
//!
//! The shape of the assertions comes from RFC 4180 and from
//! `dataset/File.TabularData.1.0.0`. No check reads a file outside the
//! repository (a rule: tests that point to files outside the project folder
//! make CI fail):
//! the one that read an external corpus was removed.

use energyml_build::tabular::{self, Dialect};

#[test]
fn a_quoted_field_holds_its_separator() {
    // RFC 4180 §2.6: a field containing the separator is in quotes.
    // This is **the** reason for this module's existence: the naive splitting of the
    // scatter reader sees one more field and shifts everything that follows.
    let text = "a,b,c\n1,\"x, y\",3\n";
    let table = tabular::read("t.csv", text, Dialect::default()).expect("reads");
    assert_eq!(table.headers, ["a", "b", "c"]);
    assert_eq!(table.rows, vec![vec!["1".to_string(), "x, y".to_string(), "3".to_string()]]);
}

#[test]
fn two_quotes_inside_a_quoted_field_are_one() {
    // RFC 4180 §2.7, and it is exactly what the corpus writes:
    // `"48° 54' 50.51""N, 2° 24' 15.30""E"`.
    let text = "a\n\"50.51\"\"N, 15.30\"\"E\"\n";
    let table = tabular::read("t.csv", text, Dialect::default()).expect("reads");
    assert_eq!(table.rows[0][0], "50.51\"N, 15.30\"E");
}

#[test]
fn the_separator_is_decided_once_and_not_per_row() {
    // The defect of the scatter reader, stated: it chooses line by line the one
    // that splits the most fields. Here the second line carries two
    // semicolons **in a quoted field**; a per-line choice would cut it
    // into three whereas the file is comma-separated.
    let text = "a,b\n1,\"x;y;z\"\n2,\"p;q;r\"\n";
    let dialect = Dialect::sniff(text);
    assert_eq!(dialect.separator, ',');
    let table = tabular::read("t.csv", text, dialect).expect("reads");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0], vec!["1".to_string(), "x;y;z".to_string()]);
}

#[test]
fn a_separator_inside_quotes_does_not_vote() {
    // The sniffing counts **outside** quotes: a line whose only quoted
    // field carries three commas does not make a comma file.
    let text = "a;b;c\n1;\"x,y,z,w\";3\n";
    assert_eq!(Dialect::sniff(text).separator, ';');
}

#[test]
fn a_semicolon_file_is_proposed_a_comma_decimal() {
    // Proposed, never imposed — and it is the only non-ambiguous combination: a
    // semicolon file that writes `1,5` is the French convention.
    // `File.TabularData` cannot say it (checked: the schema only speaks of
    // “decimal” for the fixed-width format), so it is up to us.
    let dialect = Dialect::sniff("a;b\n1,5;2,5\n");
    assert_eq!(dialect.decimal, ',');
    assert_eq!(dialect.number("1,5"), Some(1.5));
    assert_eq!(Dialect::default().number("1.5"), Some(1.5));
}

#[test]
fn a_row_that_does_not_line_up_is_refused_by_its_line() {
    // A file of fifty thousand lines is corrected by its line. And the
    // number is that of the **file**, not the rank among the retained lines:
    // here line 1 is a comment.
    let text = "# a guard\na,b,c\n1,2,3\n4,5\n";
    let why = tabular::read("t.csv", text, Dialect::default()).expect_err("refuses");
    assert!(why.why.contains("line 4"), "{}", why.why);
    assert!(why.why.contains("2 field"), "{}", why.why);
    assert!(why.why.contains('3'), "{}", why.why);
}

#[test]
fn two_columns_of_one_name_are_refused() {
    // Matching a template is done **by column name**: two
    // columns of the same name cannot be told apart, so the file is
    // refused rather than one of the two being silently ignored.
    let why = tabular::read("t.csv", "a,b,a\n1,2,3\n", Dialect::default()).expect_err("refuses");
    assert!(why.why.contains("`a`"), "{}", why.why);
}

#[test]
fn an_unclosed_quote_is_refused_by_where_it_opened() {
    let why = tabular::read("t.csv", "a,b\n1,\"x\n", Dialect::default()).expect_err("refuses");
    assert!(why.why.contains("line 2"), "{}", why.why);
}

#[test]
fn guards_and_comments_are_skipped_and_still_counted() {
    let dialect = Dialect { skip: 2, ..Dialect::default() };
    let text = "garde 1\ngarde 2\n# un commentaire\na,b\n1,2\n";
    let table = tabular::read("t.csv", text, dialect).expect("reads");
    assert_eq!(table.headers, ["a", "b"]);
    assert_eq!(table.rows.len(), 1);
}

#[test]
fn without_a_header_the_position_is_the_name() {
    // `ColumnNamesInFirstRow: false`: only the rank is left to match on,
    // so the columns are named by it.
    let dialect = Dialect { header: false, ..Dialect::default() };
    let table = tabular::read("t.csv", "1,2,3\n4,5,6\n", dialect).expect("reads");
    assert_eq!(table.headers, ["0", "1", "2"]);
    assert_eq!(table.rows.len(), 2);
}
