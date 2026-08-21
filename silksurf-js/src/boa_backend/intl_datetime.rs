/*
 * Intl.DateTimeFormat's formatting algorithm.
 *
 * ECMA-402 splits date and time formatting into option resolution, which turns
 * a locale list and an option bag into a pattern, and pattern rendering, which
 * turns a pattern and a set of calendar fields into the parts formatToParts
 * reports. Both answers a page observes byte-for-byte, so both compute here;
 * platform_globals' bootstrap carries the object shape.
 *
 * The calendar fields arrive from the bootstrap, which reads them off the Date
 * object with either the local or the UTC accessors. Intl and Date therefore
 * agree on the wall clock by construction, and no time zone database enters
 * this crate.
 */

use std::sync::OnceLock;

use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue, NativeFunction, js_string,
    object::{JsObject, ObjectInitializer, builtins::JsArray},
    property::Attribute,
};

use super::intl_datetime_data::{LOCALES, LocaleData, NONE, PATTERNS};

/// The pattern pool split once per process. `PATTERNS` stores one pattern per
/// line and the per-locale tables index this slice.
fn patterns() -> &'static [&'static str] {
    static SPLIT: OnceLock<Box<[&'static str]>> = OnceLock::new();
    SPLIT.get_or_init(|| PATTERNS.lines().collect::<Vec<_>>().into_boxed_slice())
}

fn pattern_at(index: u16) -> Option<&'static str> {
    if index == NONE {
        return None;
    }
    patterns().get(index as usize).copied()
}

// ---- option widths ----------------------------------------------------------

/// A field's requested width, encoded as the ordinal the generated signature
/// tables index by. Zero means the option bag omits the field.
#[derive(Clone, Copy, Default)]
struct Widths {
    weekday: u16,
    era: u16,
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
}

const NAME_WIDTHS: [&str; 3] = ["long", "short", "narrow"];
const NUMERIC_WIDTHS: [&str; 2] = ["numeric", "2-digit"];
const MONTH_WIDTHS: [&str; 5] = ["numeric", "2-digit", "long", "short", "narrow"];
const HOUR_CYCLES: [&str; 4] = ["h11", "h12", "h23", "h24"];

/// Read one string option, rejecting a value outside `allowed` the way
/// ECMA-402's `GetOption` does. The returned ordinal is one past the index in
/// `allowed`, leaving zero for an absent option.
fn width_option(
    options: &JsObject,
    key: &str,
    allowed: &[&str],
    context: &mut Context,
) -> JsResult<u16> {
    let value = options.get(js_string!(key), context)?;
    if value.is_undefined() {
        return Ok(0);
    }
    let text = value.to_string(context)?.to_std_string_lossy();
    allowed
        .iter()
        .position(|candidate| *candidate == text)
        .map(|index| index as u16 + 1)
        .ok_or_else(|| {
            JsError::from(JsNativeError::range().with_message(format!(
                "value {text} out of range for Intl.DateTimeFormat option {key}"
            )))
        })
}

fn read_widths(options: &JsObject, context: &mut Context) -> JsResult<Widths> {
    Ok(Widths {
        weekday: width_option(options, "weekday", &NAME_WIDTHS, context)?,
        era: width_option(options, "era", &NAME_WIDTHS, context)?,
        year: width_option(options, "year", &NUMERIC_WIDTHS, context)?,
        month: width_option(options, "month", &MONTH_WIDTHS, context)?,
        day: width_option(options, "day", &NUMERIC_WIDTHS, context)?,
        hour: width_option(options, "hour", &NUMERIC_WIDTHS, context)?,
        minute: width_option(options, "minute", &NUMERIC_WIDTHS, context)?,
        second: width_option(options, "second", &NUMERIC_WIDTHS, context)?,
    })
}

impl Widths {
    fn has_date(self) -> bool {
        self.weekday | self.era | self.year | self.month | self.day != 0
    }

    fn has_time(self) -> bool {
        self.hour | self.minute | self.second != 0
    }

    /// The dense slot the generated date table indexes by.
    fn date_signature(self) -> usize {
        (self.weekday + self.era * 4 + self.year * 16 + self.month * 48 + self.day * 288) as usize
    }

    /// The dense slot the generated time table indexes by, given an hour cycle.
    fn time_signature(self, cycle: usize) -> usize {
        let cycle = if self.hour == 0 { 0 } else { cycle };
        (self.hour + self.minute * 3 + self.second * 9) as usize + cycle * 27
    }

    /// The glue slot, which CLDR selects by the width the month resolves to
    /// and by whether the date pattern carries a weekday.
    fn glue_slot(self) -> usize {
        (self.month * 2 + u16::from(self.weekday != 0)) as usize
    }
}

const STYLE_NAMES: [&str; 4] = ["full", "long", "medium", "short"];

/// The style shorthands a request names. ECMA-402 forbids mixing a style with
/// a component option and has `resolvedOptions` report the style rather than
/// the components it resolved to, so a style carries its own pattern slot
/// instead of expanding into widths.
#[derive(Clone, Copy, Default)]
struct Styles {
    date: u16,
    time: u16,
}

impl Styles {
    fn named(self) -> bool {
        self.date != 0 || self.time != 0
    }

    /// The dense slot the generated style table indexes by.
    fn signature(self, cycle: usize) -> usize {
        let cycle = if self.time == 0 { 0 } else { cycle };
        (self.date as usize * 5 + self.time as usize) * 4 + cycle
    }
}

fn read_styles(options: &JsObject, widths: Widths, context: &mut Context) -> JsResult<Styles> {
    let styles = Styles {
        date: width_option(options, "dateStyle", &STYLE_NAMES, context)?,
        time: width_option(options, "timeStyle", &STYLE_NAMES, context)?,
    };
    if styles.named() && (widths.has_date() || widths.has_time()) {
        return Err(JsNativeError::typ()
            .with_message("Intl.DateTimeFormat takes either a style or component options")
            .into());
    }
    Ok(styles)
}

// ---- locale negotiation -----------------------------------------------------

/// Match a requested tag against the supported set, dropping one subtag at a
/// time the way ECMA-402's `BestAvailableLocale` does.
fn best_available(tag: &str) -> Option<&'static LocaleData> {
    let mut candidate = tag.to_string();
    loop {
        if let Some(found) = LOCALES
            .iter()
            .find(|entry| entry.tag.eq_ignore_ascii_case(&candidate))
        {
            return Some(found);
        }
        match candidate.rfind('-') {
            Some(cut) => candidate.truncate(cut),
            None => return None,
        }
    }
}

/// Resolve a requested locale list to a supported entry, falling back to the
/// first supported locale so `resolvedOptions().locale` always names the
/// locale that actually formatted the value.
fn negotiate(tags: &[String]) -> &'static LocaleData {
    tags.iter()
        .find_map(|tag| best_available(tag))
        .unwrap_or(&LOCALES[0])
}

fn requested_tags(value: &JsValue, context: &mut Context) -> JsResult<Vec<String>> {
    if value.is_undefined() || value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(object) = value.as_object()
        && object.is_array()
    {
        let array = JsArray::from_object(object.clone())?;
        let length = array.length(context)?;
        let mut tags = Vec::with_capacity(length as usize);
        for index in 0..length {
            let item = array.get(index, context)?;
            tags.push(item.to_string(context)?.to_std_string_lossy());
        }
        return Ok(tags);
    }
    Ok(vec![value.to_string(context)?.to_std_string_lossy()])
}

// ---- time zone --------------------------------------------------------------

/// The zone the host clock runs in, read from `TZ` or from the zoneinfo path
/// `/etc/localtime` resolves to. `Date`'s own accessors carry the offset, so this
/// name is what `resolvedOptions().timeZone` reports rather than an input to
/// formatting.
pub(super) fn host_time_zone() -> &'static str {
    static ZONE: OnceLock<String> = OnceLock::new();
    ZONE.get_or_init(|| {
        if let Ok(tz) = std::env::var("TZ")
            && let tz = tz.trim_start_matches(':')
            && tz.contains('/')
        {
            return tz.to_string();
        }
        let Ok(target) = std::fs::read_link("/etc/localtime") else {
            return "UTC".to_string();
        };
        let text = target.to_string_lossy();
        match text.split_once("zoneinfo/") {
            Some((_, zone)) if !zone.is_empty() => zone.to_string(),
            _ => "UTC".to_string(),
        }
    })
}

/// Accept the zone the engine can actually render: UTC, its aliases, and the
/// host's own zone, which is the value `resolvedOptions().timeZone` hands back
/// to a page that reads it and passes it to a second formatter. Any other
/// named zone needs an offset this build cannot compute, so it reports the
/// `RangeError` ECMA-402 specifies rather than formatting the wrong instant.
fn resolve_time_zone(options: &JsObject, context: &mut Context) -> JsResult<(String, bool)> {
    let value = options.get(js_string!("timeZone"), context)?;
    if value.is_undefined() {
        return Ok((host_time_zone().to_string(), false));
    }
    let text = value.to_string(context)?.to_std_string_lossy();
    if text.eq_ignore_ascii_case("utc") || text.eq_ignore_ascii_case("etc/utc") {
        return Ok(("UTC".to_string(), true));
    }
    if text.eq_ignore_ascii_case(host_time_zone()) {
        return Ok((host_time_zone().to_string(), false));
    }
    Err(JsNativeError::range()
        .with_message(format!(
            "time zone {text} is outside this build's zone data"
        ))
        .into())
}

// ---- resolution -------------------------------------------------------------

/// `__silksurfIntlDateTimeResolve(locales, options)`: perform ECMA-402's
/// `InitializeDateTimeFormat` and hand the bootstrap the resolved record plus
/// the pattern that renders it.
/// The pattern a resolved request renders with, taken from the style table
/// when the request names a style and composed from the component tables
/// otherwise.
fn resolved_pattern(
    locale: &'static LocaleData,
    widths: Widths,
    styles: Styles,
    cycle: usize,
) -> JsResult<String> {
    if styles.named() {
        return pattern_at(locale.styles[styles.signature(cycle)])
            .map(str::to_string)
            .ok_or_else(unsupported);
    }
    compose(locale, widths, cycle)
}

/// Build the record the bootstrap reads: the fields `resolvedOptions` reports,
/// the pattern, and whether the fields come from the UTC accessors.
fn build_record(
    locale: &'static LocaleData,
    zone: &str,
    utc: bool,
    pattern: &str,
    cycle: Option<usize>,
    context: &mut Context,
) -> JsObject {
    let mut record = ObjectInitializer::new(context);
    record
        .property(
            js_string!("locale"),
            js_string!(locale.tag),
            Attribute::all(),
        )
        .property(
            js_string!("calendar"),
            js_string!("gregory"),
            Attribute::all(),
        )
        .property(
            js_string!("numberingSystem"),
            js_string!("latn"),
            Attribute::all(),
        )
        .property(js_string!("timeZone"), js_string!(zone), Attribute::all())
        .property(js_string!("utc"), JsValue::from(utc), Attribute::all())
        .property(js_string!("pattern"), js_string!(pattern), Attribute::all());
    if let Some(cycle) = cycle {
        record.property(
            js_string!("hourCycle"),
            js_string!(HOUR_CYCLES[cycle]),
            Attribute::all(),
        );
        record.property(
            js_string!("hour12"),
            JsValue::from(cycle < 2),
            Attribute::all(),
        );
    }
    record.build()
}

fn resolve(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let tags = requested_tags(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let locale = negotiate(&tags);
    let options = options_object(args.get(1), context)?;

    let mut widths = read_widths(&options, context)?;
    let styles = read_styles(&options, widths, context)?;
    // A bag naming no field formats the date, which is what a bare
    // `new Intl.DateTimeFormat().format(d)` renders.
    if !styles.named() && !widths.has_date() && !widths.has_time() {
        widths.year = 1;
        widths.month = 1;
        widths.day = 1;
    }

    let (zone, utc) = resolve_time_zone(&options, context)?;
    let has_time = if styles.named() {
        styles.time != 0
    } else {
        widths.has_time()
    };
    let cycle = hour_cycle(&options, locale, has_time, context)?;
    let pattern = resolved_pattern(locale, widths, styles, cycle)?;

    let record = build_record(
        locale,
        &zone,
        utc,
        &pattern,
        has_time.then_some(cycle),
        context,
    );
    if styles.named() {
        write_styles(&record, styles, context)?;
    } else {
        write_widths(&record, widths, context)?;
    }
    Ok(record.into())
}

fn options_object(value: Option<&JsValue>, context: &mut Context) -> JsResult<JsObject> {
    match value {
        Some(value) if !value.is_undefined() && !value.is_null() => value.to_object(context),
        _ => Ok(JsObject::with_null_proto()),
    }
}

/// Report the widths back in the names ECMA-402 gives them, so
/// `resolvedOptions()` reads them straight off the record.
fn write_widths(record: &JsObject, widths: Widths, context: &mut Context) -> JsResult<()> {
    let fields: [(&str, u16, &[&str]); 8] = [
        ("weekday", widths.weekday, &NAME_WIDTHS),
        ("era", widths.era, &NAME_WIDTHS),
        ("year", widths.year, &NUMERIC_WIDTHS),
        ("month", widths.month, &MONTH_WIDTHS),
        ("day", widths.day, &NUMERIC_WIDTHS),
        ("hour", widths.hour, &NUMERIC_WIDTHS),
        ("minute", widths.minute, &NUMERIC_WIDTHS),
        ("second", widths.second, &NUMERIC_WIDTHS),
    ];
    for (name, width, allowed) in fields {
        if width == 0 {
            continue;
        }
        record.set(
            js_string!(name),
            js_string!(allowed[width as usize - 1]),
            true,
            context,
        )?;
    }
    Ok(())
}

/// Report the style names back, which is what `resolvedOptions` carries for a
/// formatter a style built.
fn write_styles(record: &JsObject, styles: Styles, context: &mut Context) -> JsResult<()> {
    for (name, style) in [("dateStyle", styles.date), ("timeStyle", styles.time)] {
        if style == 0 {
            continue;
        }
        record.set(
            js_string!(name),
            js_string!(STYLE_NAMES[style as usize - 1]),
            true,
            context,
        )?;
    }
    Ok(())
}

/// The hour cycle the request resolves to: an explicit hourCycle, else hour12,
/// else the locale's own default.
fn hour_cycle(
    options: &JsObject,
    locale: &LocaleData,
    has_time: bool,
    context: &mut Context,
) -> JsResult<usize> {
    if !has_time {
        return Ok(0);
    }
    let hour12 = options.get(js_string!("hour12"), context)?;
    if !hour12.is_undefined() {
        return Ok(if hour12.to_boolean() { 1 } else { 2 });
    }
    let named = width_option(options, "hourCycle", &HOUR_CYCLES, context)?;
    if named != 0 {
        return Ok(named as usize - 1);
    }
    Ok(HOUR_CYCLES
        .iter()
        .position(|cycle| *cycle == locale.hour_cycle)
        .unwrap_or(2))
}

/// The error a field combination outside the generated tables reports.
fn unsupported() -> JsError {
    JsError::from(
        JsNativeError::range()
            .with_message("Intl.DateTimeFormat has no pattern for this field combination"),
    )
}

/// Join the date and time patterns the widths select. A combination the
/// reference implementation rejects has no slot, and the caller reports it.
fn compose(locale: &LocaleData, widths: Widths, cycle: usize) -> JsResult<String> {
    let date = widths
        .has_date()
        .then(|| pattern_at(locale.dates[widths.date_signature()]));
    let time = widths
        .has_time()
        .then(|| pattern_at(locale.times[widths.time_signature(cycle)]));
    match (date, time) {
        (Some(date), Some(time)) => {
            let (date, time) = (date.ok_or_else(unsupported)?, time.ok_or_else(unsupported)?);
            let glue = pattern_at(locale.glue[widths.glue_slot()]).unwrap_or(", ");
            Ok(format!("{date}{glue}{time}"))
        }
        (Some(date), None) => Ok(date.ok_or_else(unsupported)?.to_string()),
        (None, Some(time)) => Ok(time.ok_or_else(unsupported)?.to_string()),
        (None, None) => Err(unsupported()),
    }
}

// ---- rendering --------------------------------------------------------------

/// The calendar fields the bootstrap reads off the Date object, in the order
/// it passes them.
#[derive(Clone, Copy, Default)]
struct Fields {
    year: i64,
    month: i64,
    day: i64,
    weekday: i64,
    hour: i64,
    minute: i64,
    second: i64,
    millisecond: i64,
}

fn pad(value: i64, width: usize) -> String {
    format!("{value:0width$}")
}

/// Render one pattern token. The token's letter names the field and its length
/// names the width, which is how a CLDR pattern encodes both.
fn render_token(
    letter: char,
    count: usize,
    fields: Fields,
    locale: &LocaleData,
) -> (&'static str, String) {
    let name_index = |count: usize| match count {
        4 => 0,
        5 => 2,
        _ => 1,
    };
    match letter {
        'y' => {
            // Year zero is 1 BC, so the era year counts back from one.
            let era_year = if fields.year <= 0 {
                1 - fields.year
            } else {
                fields.year
            };
            let text = if count == 2 {
                pad(era_year % 100, 2)
            } else {
                era_year.to_string()
            };
            ("year", text)
        }
        'M' => match count {
            1 | 2 => ("month", pad(fields.month + 1, count)),
            _ => (
                "month",
                locale.months[name_index(count)][fields.month as usize % 12].to_string(),
            ),
        },
        'd' => ("day", pad(fields.day, count)),
        'E' => (
            "weekday",
            locale.weekdays[name_index(count)][fields.weekday as usize % 7].to_string(),
        ),
        'G' => {
            let era = usize::from(fields.year > 0);
            let index = match count {
                4 => 0,
                5 => 2,
                _ => 1,
            };
            ("era", locale.eras[index][era].to_string())
        }
        'h' => ("hour", pad(hour12(fields.hour), count)),
        'K' => ("hour", pad(fields.hour % 12, count)),
        'H' => ("hour", pad(fields.hour, count)),
        'k' => (
            "hour",
            pad(if fields.hour == 0 { 24 } else { fields.hour }, count),
        ),
        'm' => ("minute", pad(fields.minute, count)),
        's' => ("second", pad(fields.second, count)),
        'S' => ("fractionalSecond", fractional(fields.millisecond, count)),
        'a' => (
            "dayPeriod",
            locale.day_periods[usize::from(fields.hour >= 12)].to_string(),
        ),
        _ => ("literal", String::new()),
    }
}

fn hour12(hour: i64) -> i64 {
    match hour % 12 {
        0 => 12,
        other => other,
    }
}

/// Milliseconds, widened or truncated to the digit count the pattern asks for.
fn fractional(millisecond: i64, count: usize) -> String {
    let text = pad(millisecond, 3);
    match count {
        0 | 3 => text,
        n if n < 3 => text[..n].to_string(),
        n => format!("{text}{}", "0".repeat(n - 3)),
    }
}

/// Walk a pattern, emitting one part per token run and one per literal run.
/// A quoted run is literal text, and a doubled quote inside it is one quote.
fn render(pattern: &str, fields: Fields, locale: &LocaleData) -> Vec<(&'static str, String)> {
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    let mut literal = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        if current == '\'' {
            index += 1;
            while index < chars.len() {
                if chars[index] == '\'' {
                    if chars.get(index + 1) == Some(&'\'') {
                        literal.push('\'');
                        index += 2;
                        continue;
                    }
                    index += 1;
                    break;
                }
                literal.push(chars[index]);
                index += 1;
            }
            continue;
        }
        if !current.is_ascii_alphabetic() {
            literal.push(current);
            index += 1;
            continue;
        }
        let mut count = 0;
        while index + count < chars.len() && chars[index + count] == current {
            count += 1;
        }
        index += count;
        let (kind, text) = render_token(current, count, fields, locale);
        if kind == "literal" {
            literal.push_str(&text);
            continue;
        }
        if !literal.is_empty() {
            parts.push(("literal", std::mem::take(&mut literal)));
        }
        parts.push((kind, text));
    }
    if !literal.is_empty() {
        parts.push(("literal", literal));
    }
    parts
}

/// `__silksurfIntlDateTimeParts(pattern, locale, fields)`: render the parts
/// `formatToParts` reports and `format` joins.
fn format_parts(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let pattern = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_string(context)?
        .to_std_string_lossy();
    let tag = args
        .get(1)
        .unwrap_or(&JsValue::undefined())
        .to_string(context)?
        .to_std_string_lossy();
    let locale = negotiate(&[tag]);
    let fields = read_fields(args.get(2), context)?;

    let array = JsArray::new(context);
    for (kind, text) in render(&pattern, fields, locale) {
        let part = ObjectInitializer::new(context)
            .property(js_string!("type"), js_string!(kind), Attribute::all())
            .property(js_string!("value"), js_string!(text), Attribute::all())
            .build();
        array.push(part, context)?;
    }
    Ok(array.into())
}

fn read_fields(value: Option<&JsValue>, context: &mut Context) -> JsResult<Fields> {
    let Some(object) = value.and_then(JsValue::as_object) else {
        return Ok(Fields::default());
    };
    let array = JsArray::from_object(object.clone())?;
    let mut slots = [0_i64; 8];
    for (index, slot) in slots.iter_mut().enumerate() {
        *slot = array.get(index as u64, context)?.to_number(context)? as i64;
    }
    Ok(Fields {
        year: slots[0],
        month: slots[1],
        day: slots[2],
        weekday: slots[3],
        hour: slots[4],
        minute: slots[5],
        second: slots[6],
        millisecond: slots[7],
    })
}

/// `__silksurfIntlSupportedLocales(locales)`: the subset of a request this
/// build formats, which is what `supportedLocalesOf` reports.
fn supported_locales(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let tags = requested_tags(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let array = JsArray::new(context);
    for tag in tags {
        if best_available(&tag).is_some() {
            array.push(js_string!(tag), context)?;
        }
    }
    Ok(array.into())
}

/// Install the natives the bootstrap's Intl.DateTimeFormat calls.
pub(super) fn install(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("__silksurfIntlDateTimeResolve"),
        2,
        NativeFunction::from_fn_ptr(resolve),
    );
    let _ = ctx.register_global_callable(
        js_string!("__silksurfIntlDateTimeParts"),
        3,
        NativeFunction::from_fn_ptr(format_parts),
    );
    let _ = ctx.register_global_callable(
        js_string!("__silksurfIntlSupportedLocales"),
        1,
        NativeFunction::from_fn_ptr(supported_locales),
    );
}
