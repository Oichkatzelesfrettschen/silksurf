/*
 * Intl.DateTimeFormat checked against the reference ECMA-402 implementation.
 *
 * fixtures/intl_datetime_vectors.js carries one row per option bag: the text
 * the reference implementation renders for a named locale and instant. The
 * date and time signatures are enumerated in full, so a disagreement in any
 * pattern slot the tables carry fails here rather than on a page.
 */

const VECTORS: &str = include_str!("fixtures/intl_datetime_vectors.js");

const HARNESS: &str = r"
(function () {
    var WEEKDAY = [null, 'long', 'short', 'narrow'];
    var ERA = [null, 'long', 'short', 'narrow'];
    var YEAR = [null, 'numeric', '2-digit'];
    var MONTH = [null, 'numeric', '2-digit', 'long', 'short', 'narrow'];
    var NUM2 = [null, 'numeric', '2-digit'];
    var CYCLES = ['h11', 'h12', 'h23', 'h24'];
    var STYLES = [null, 'full', 'long', 'medium', 'short'];
    function bag(code) {
        var c = function (i) { return code[i] || 0; };
        var o = { timeZone: 'UTC' };
        if (c(0)) { o.weekday = WEEKDAY[c(0)]; }
        if (c(1)) { o.era = ERA[c(1)]; }
        if (c(2)) { o.year = YEAR[c(2)]; }
        if (c(3)) { o.month = MONTH[c(3)]; }
        if (c(4)) { o.day = NUM2[c(4)]; }
        if (c(5)) { o.hour = NUM2[c(5)]; }
        if (c(6)) { o.minute = NUM2[c(6)]; }
        if (c(7)) { o.second = NUM2[c(7)]; }
        if (c(8)) { o.hourCycle = CYCLES[c(8) - 1]; }
        if (c(9)) { o.dateStyle = STYLES[c(9)]; }
        if (c(10)) { o.timeStyle = STYLES[c(10)]; }
        return o;
    }
    var rows = globalThis.__intlDateTimeVectors;
    var locales = globalThis.__intlDateTimeLocales;
    var failures = [];
    for (var i = 0; i < rows.length; i++) {
        var row = rows[i], locale = locales[row[0]], options = bag(row[1]);
        var got;
        try { got = new Intl.DateTimeFormat(locale, options).format(row[2]); }
        catch (e) { got = 'THREW ' + e; }
        if (got !== row[3]) {
            failures.push(locale + ' ' + JSON.stringify(options) + ' @' + row[2]
                + ' want ' + JSON.stringify(row[3]) + ' got ' + JSON.stringify(got));
        }
    }
    if (failures.length) {
        throw new Error(failures.length + ' of ' + rows.length + ' vectors disagree:\n'
            + failures.slice(0, 20).join('\n'));
    }
    console.log('intl_datetime_conformance: ' + rows.length + ' vectors agree');
})();
";

#[test]
fn every_vector_matches_the_reference_implementation() {
    let mut ctx = silksurf_js::SilkContext::new();
    ctx.eval(VECTORS).expect("vector fixture loads");
    ctx.eval(HARNESS).expect("every vector agrees");
}
