/*
 * Intl.DateTimeFormat's object behavior: negotiation, resolved options, the
 * detachable format accessor, the errors ECMA-402 specifies, and the Date
 * methods that delegate to it.
 *
 * intl_datetime_conformance.rs covers the rendered text against the reference
 * implementation; these cases cover the shape around it, which no rendered
 * string reveals.
 */

fn run(script: &str) {
    let mut ctx = silksurf_js::SilkContext::new();
    ctx.eval(script).expect("script runs without throwing");
}

/// Assert helper shared by every case, so a failure names the value it saw.
const ASSERT: &str = r"
function eq(got, want, label) {
    if (got !== want) { throw new Error(label + ': want ' + JSON.stringify(want) + ' got ' + JSON.stringify(got)); }
}
function threw(fn, kind, label) {
    try { fn(); } catch (e) { if (e instanceof kind) { return; } throw new Error(label + ': threw ' + e); }
    throw new Error(label + ': did not throw');
}
var UTC = { timeZone: 'UTC' };
var T = Date.UTC(2026, 7, 20, 14, 5, 9, 123);
";

#[test]
fn resolved_options_reports_the_negotiated_locale_and_the_requested_components() {
    run(&format!(
        "{ASSERT}
        var r = new Intl.DateTimeFormat(['fr-CA', 'en-GB'], {{ timeZone: 'UTC', year: 'numeric', month: 'short' }}).resolvedOptions();
        eq(r.locale, 'en-GB', 'locale');
        eq(r.calendar, 'gregory', 'calendar');
        eq(r.numberingSystem, 'latn', 'numberingSystem');
        eq(r.timeZone, 'UTC', 'timeZone');
        eq(r.year, 'numeric', 'year');
        eq(r.month, 'short', 'month');
        eq(r.day, undefined, 'day stays absent');
        eq(r.hourCycle, undefined, 'a date-only request resolves no hour cycle');
        eq(Object.keys(r).join(','), 'locale,calendar,numberingSystem,timeZone,year,month', 'key order');"
    ));
}

#[test]
fn resolved_options_reports_a_style_rather_than_the_components_it_resolved_to() {
    run(&format!(
        "{ASSERT}
        var r = new Intl.DateTimeFormat('en-US', {{ timeZone: 'UTC', dateStyle: 'long', timeStyle: 'short' }}).resolvedOptions();
        eq(r.dateStyle, 'long', 'dateStyle');
        eq(r.timeStyle, 'short', 'timeStyle');
        eq(r.year, undefined, 'a style reports no year component');
        eq(r.hourCycle, 'h12', 'hourCycle');
        eq(r.hour12, true, 'hour12');"
    ));
}

#[test]
fn format_is_an_accessor_whose_bound_function_survives_detachment() {
    run(&format!(
        "{ASSERT}
        var f = new Intl.DateTimeFormat('en-US', UTC);
        var detached = f.format;
        eq(detached(T), '8/20/2026', 'detached call');
        eq(detached, f.format, 'the accessor returns one bound function');
        eq([T, T].map(detached).join('|'), '8/20/2026|8/20/2026', 'used as a map callback');"
    ));
}

#[test]
fn an_unsupported_locale_negotiates_to_a_locale_the_page_can_read_back() {
    run(&format!(
        "{ASSERT}
        eq(new Intl.DateTimeFormat('fr', UTC).resolvedOptions().locale, 'en-US', 'fallback');
        eq(new Intl.DateTimeFormat('en-GB-oxendict', UTC).resolvedOptions().locale, 'en-GB', 'subtag truncation');
        eq(Intl.DateTimeFormat.supportedLocalesOf(['fr', 'en-GB', 'de-DE', 'en-US']).join(','), 'en-GB,en-US', 'supportedLocalesOf');
        eq(Intl.DateTimeFormat.supportedLocalesOf([]).length, 0, 'an empty request supports nothing');"
    ));
}

#[test]
fn a_time_zone_outside_the_build_reports_a_range_error() {
    run(&format!(
        "{ASSERT}
        var host = new Intl.DateTimeFormat().resolvedOptions().timeZone;
        eq(typeof host, 'string', 'the host zone has a name');
        eq(new Intl.DateTimeFormat('en-US', {{ timeZone: host }}).resolvedOptions().timeZone, host, 'the reported zone round-trips');
        eq(new Intl.DateTimeFormat('en-US', {{ timeZone: 'utc' }}).resolvedOptions().timeZone, 'UTC', 'UTC canonicalizes');
        threw(function () {{ new Intl.DateTimeFormat('en-US', {{ timeZone: 'Antarctica/Troll' }}); }}, RangeError, 'unknown zone');
        threw(function () {{ new Intl.DateTimeFormat('en-US', {{ month: 'wide' }}); }}, RangeError, 'unknown option value');
        threw(function () {{ new Intl.DateTimeFormat('en-US', {{ dateStyle: 'long', month: 'short' }}); }}, TypeError, 'style mixed with a component');"
    ));
}

#[test]
fn the_default_zone_agrees_with_the_date_object_it_formats() {
    // Date carries the host offset, so a formatter that names no zone reads the
    // same wall clock the Date accessors report.
    run(&format!(
        "{ASSERT}
        var d = new Date(T);
        var parts = new Intl.DateTimeFormat('en-US', {{ hour: 'numeric', hourCycle: 'h23' }}).formatToParts(T);
        var hour = parts.filter(function (p) {{ return p.type === 'hour'; }})[0].value;
        eq(Number(hour), d.getHours(), 'the local hour matches Date');
        var utc = new Intl.DateTimeFormat('en-US', {{ timeZone: 'UTC', hour: 'numeric', hourCycle: 'h23' }}).formatToParts(T);
        eq(Number(utc.filter(function (p) {{ return p.type === 'hour'; }})[0].value), d.getUTCHours(), 'the UTC hour matches Date');
        // The reported zone names the zone the accessors read. A fixed-offset
        // identifier states its own offset, so that pairing is checkable here;
        // a named zone's is not. Neither branch fires on a host whose
        // /etc/localtime is a zoneinfo symlink, which is where this runs, so
        // this guards the container and TZ configurations rather than this one.
        var zone = new Intl.DateTimeFormat().resolvedOptions().timeZone;
        var offset = -d.getTimezoneOffset();
        if (zone === 'UTC') {{ eq(offset, 0, 'a UTC report needs a zero offset'); }}
        var etc = /^Etc[/]GMT([+-])([0-9]{{1,2}})$/.exec(zone);
        if (etc) {{ eq(offset, (etc[1] === '+' ? -60 : 60) * Number(etc[2]), 'an Etc/GMT report states its offset'); }}"
    ));
}

#[test]
fn date_locale_methods_fill_in_the_defaults_for_the_half_they_name() {
    run(&format!(
        "{ASSERT}
        var d = new Date(T);
        eq(d.toLocaleDateString('en-US', UTC), '8/20/2026', 'toLocaleDateString');
        eq(d.toLocaleTimeString('en-US', UTC), '2:05:09 PM', 'toLocaleTimeString');
        eq(d.toLocaleString('en-US', UTC), '8/20/2026, 2:05:09 PM', 'toLocaleString');
        eq(d.toLocaleDateString('en-GB', UTC), '20/08/2026', 'en-GB date order');
        eq(d.toLocaleDateString('en-US', {{ timeZone: 'UTC', month: 'long' }}), 'August', 'a named component overrides the defaults');
        eq(new Date(NaN).toLocaleDateString(), 'Invalid Date', 'an invalid date formats as Invalid Date');"
    ));
}

#[test]
fn format_reports_a_range_error_for_a_time_value_outside_the_representable_range() {
    run(&format!(
        "{ASSERT}
        threw(function () {{ new Intl.DateTimeFormat('en-US', UTC).format(NaN); }}, RangeError, 'NaN');
        threw(function () {{ new Intl.DateTimeFormat('en-US', UTC).format(Infinity); }}, RangeError, 'Infinity');
        eq(typeof new Intl.DateTimeFormat('en-US', UTC).format(), 'string', 'an omitted date formats now');"
    ));
}

#[test]
fn a_year_before_the_common_era_counts_back_from_one() {
    run(&format!(
        "{ASSERT}
        var bc = Date.UTC(-1, 5, 15);
        var o = {{ timeZone: 'UTC', era: 'short', year: 'numeric' }};
        eq(new Intl.DateTimeFormat('en-US', o).format(bc), '2 BC', 'year -1 is 2 BC');
        eq(new Intl.DateTimeFormat('en-US', o).format(Date.UTC(2026, 0, 1)), '2026 AD', 'a common era year');"
    ));
}

#[test]
fn the_hour_cycle_follows_hour12_then_hour_cycle_then_the_locale() {
    run(&format!(
        "{ASSERT}
        var at = function (locale, opts) {{
            return new Intl.DateTimeFormat(locale, Object.assign({{ timeZone: 'UTC', hour: 'numeric' }}, opts)).format(T);
        }};
        eq(at('en-US', {{}}), '2 PM', 'en-US defaults to h12');
        eq(at('en-GB', {{}}), '14', 'en-GB defaults to h23');
        eq(at('en-US', {{ hour12: false }}), '14', 'hour12 false');
        eq(at('en-GB', {{ hour12: true }}), '2 pm', 'en-GB spells the day period in lower case');
        eq(at('en-US', {{ hourCycle: 'h23' }}), '14', 'an explicit cycle');
        eq(at('en-US', {{ hour12: true, hourCycle: 'h23' }}), '2 PM', 'hour12 outranks hourCycle');"
    ));
}

#[test]
fn a_style_that_carries_a_zone_name_renders_one_rather_than_reporting_an_error() {
    run(&format!(
        "{ASSERT}
        eq(new Intl.DateTimeFormat('en-US', {{ timeZone: 'UTC', timeStyle: 'long' }}).format(T), '2:05:09 PM UTC', 'timeStyle long');
        eq(new Intl.DateTimeFormat('en-US', {{ timeZone: 'UTC', timeStyle: 'full' }}).format(T), '2:05:09 PM Coordinated Universal Time', 'timeStyle full');
        eq(new Date(T).toLocaleTimeString('en-US', {{ timeZone: 'UTC', timeStyle: 'long' }}), '2:05:09 PM UTC', 'through toLocaleTimeString');
        // A zone with no name data renders the GMT offset format CLDR falls
        // back to, so the host zone formats rather than reporting an error.
        var local = new Intl.DateTimeFormat('en-US', {{ timeStyle: 'long' }}).formatToParts(T);
        var zone = local.filter(function (p) {{ return p.type === 'timeZoneName'; }})[0].value;
        eq(/^(UTC|GMT[+-][0-9]{{1,2}}(:[0-9]{{2}})?)$/.test(zone), true, 'host zone name: ' + zone);"
    ));
}

#[test]
fn format_to_parts_labels_every_piece_and_joins_back_to_format() {
    run(&format!(
        "{ASSERT}
        var f = new Intl.DateTimeFormat('en-US', {{ timeZone: 'UTC', year: 'numeric', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }});
        var parts = f.formatToParts(T);
        eq(parts.map(function (p) {{ return p.type; }}).join(','),
            'month,literal,day,literal,year,literal,hour,literal,minute,literal,dayPeriod', 'part types');
        eq(parts.map(function (p) {{ return p.value; }}).join(''), f.format(T), 'the parts join back to format');"
    ));
}
