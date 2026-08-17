//! Differential tests ported from mercury's own test suites
//! (`cleaners/resolve-split-title.test.js`, `cleaners/title.test.js`,
//! `cleaners/author.test.js`, `extractors/generic/date-published/`):
//! the reference's exact inputs and expected outputs.

use nws::mercury;

#[test]
fn resolve_split_title_normal_title_unchanged() {
    assert_eq!(
        mercury::resolve_split_title("This Is a Normal Title", ""),
        "This Is a Normal Title"
    );
}

#[test]
fn resolve_split_title_breadcrumb() {
    // mercury: 'The Best Gadgets on Earth : Bits : Blogs : NYTimes.com'
    // → 'The Best Gadgets on Earth '
    let resolved =
        mercury::resolve_split_title("The Best Gadgets on Earth : Bits : Blogs : NYTimes.com", "");
    assert_eq!(resolved.trim(), "The Best Gadgets on Earth");
}

#[test]
fn resolve_split_title_domain_at_front() {
    let resolved = mercury::resolve_split_title(
        "NYTimes - The Best Gadgets on Earth",
        "https://www.nytimes.com/bits/blog/etc/",
    );
    assert_eq!(resolved, "The Best Gadgets on Earth");
}

#[test]
fn resolve_split_title_domain_at_back() {
    let resolved = mercury::resolve_split_title(
        "The Best Gadgets on Earth | NYTimes",
        "https://www.nytimes.com/bits/blog/etc/",
    );
    assert_eq!(resolved, "The Best Gadgets on Earth");
}

#[test]
fn clean_title_strips_tags() {
    let cleaned = mercury::clean_title("This Is the <em>Real</em> Title", "", None);
    assert_eq!(cleaned, "This Is the Real Title");
}

#[test]
fn clean_title_trims_spaces() {
    let cleaned = mercury::clean_title(" This Is a Great Title That You'll Love ", "", None);
    assert_eq!(cleaned, "This Is a Great Title That You'll Love");
}

#[test]
fn clean_author_removes_by() {
    assert_eq!(mercury::clean_author("By Bob Dylan"), "Bob Dylan");
}

#[test]
fn clean_author_trims_whitespace_and_linebreaks() {
    let text = "\n      written by\n      Bob Dylan\n    ";
    assert_eq!(mercury::clean_author(text), "Bob Dylan");
}

#[test]
fn clean_date_published_basic_formats() {
    // mercury: new Date('2012/08/01').toISOString() → a valid date
    assert_eq!(
        mercury::clean_date_published("2012/08/01"),
        Some(chrono::NaiveDate::from_ymd_opt(2012, 8, 1).unwrap())
    );
    // mercury: null for unparseable input
    assert_eq!(mercury::clean_date_published("not a date"), None);
}

#[test]
fn clean_date_published_relative() {
    // mercury converts "3 days ago" via moment().subtract — today minus 3.
    let expected = chrono::Utc::now().date_naive() - chrono::Duration::days(3);
    assert_eq!(mercury::clean_date_published("3 days ago"), Some(expected));
}

#[test]
fn clean_date_published_epochs() {
    // milliseconds and seconds since epoch
    let ms = chrono::DateTime::from_timestamp_millis(1_700_000_000_000)
        .unwrap()
        .date_naive();
    assert_eq!(mercury::clean_date_published("1700000000000"), Some(ms));
    let s = chrono::DateTime::from_timestamp(1_700_000_000, 0)
        .unwrap()
        .date_naive();
    assert_eq!(mercury::clean_date_published("1700000000"), Some(s));
}
