/*
 * Emit silksurf-js/src/boa_backend/intl_datetime_data.rs.
 *
 * Every pattern is read back out of Intl.DateTimeFormat.formatToParts, which
 * labels each rendered piece with its field type, so a probe instant whose
 * fields are individually identifiable turns a rendered string into the
 * pattern that produced it. The option matrix is enumerated rather than
 * derived: ICU's skeleton width adjustment and its appendItems fallback
 * disagree with every composition rule tried against it, and an enumerated
 * table is exact by construction at 864 date slots and 108 time slots per
 * locale.
 */
const PROBE = Date.UTC(2026, 0, 3, 4, 5, 6, 70); // Sat 2026-01-03 04:05:06.070
const WEEKDAY = [null,'long','short','narrow'];
const ERA = [null,'long','short','narrow'];
const YEAR = [null,'numeric','2-digit'];
const MONTH = [null,'numeric','2-digit','long','short','narrow'];
const DAY = [null,'numeric','2-digit'];
const NUM2 = [null,'numeric','2-digit'];
const CYCLES = ['h11','h12','h23','h24'];
let HINT = {}, NAMES = null;
/*
 * Recover a named field's width from the text it rendered. A style request
 * carries no component option to read the width off, so the width is whichever
 * of the locale's three name tables holds the rendered value.
 */
function widthFromName(field, value) {
  if (!NAMES || !NAMES[field]) return null;
  const order = ['long', 'short', 'narrow'];
  for (let i = 0; i < 3; i++) if (NAMES[field][i].includes(value)) return order[i];
  return null;
}
function token(type, value, hc) {
  const two = value.length >= 2 && value[0] === '0';
  switch (type) {
    case 'year': return value.length <= 2 ? 'yy' : 'y';
    case 'month': return /^[0-9]+$/.test(value) ? (two?'MM':'M')
      : {long:'MMMM',short:'MMM',narrow:'MMMMM'}[HINT.month || widthFromName('month', value)];
    case 'day': return two?'dd':'d';
    case 'weekday': return {long:'EEEE',short:'EEE',narrow:'EEEEE'}[HINT.weekday || widthFromName('weekday', value)];
    case 'era': return {long:'GGGG',short:'G',narrow:'GGGGG'}[HINT.era || widthFromName('era', value)];
    case 'hour': return {h11:two?'KK':'K',h12:two?'hh':'h',h23:two?'HH':'H',h24:two?'kk':'k'}[hc];
    case 'minute': return two?'mm':'m';
    case 'second': return two?'ss':'s';
    case 'fractionalSecond': return 'S'.repeat(value.length);
    case 'dayPeriod': return 'a';
    /*
     * A style carries the zone name without naming a width, so the width is
     * whichever of the locale's two UTC names the value matches. CLDR falls
     * back to the GMT offset format for a zone it has no name for, which is
     * what the runtime renders for the host zone.
     */
    case 'timeZoneName': return NAMES && NAMES.zone[1] === value ? 'zzzz' : 'z';
    default: return null;
  }
}
/*
 * Node 22 returns U+202F from formatToParts where its own format returns
 * U+0020 for the same formatter, which contradicts ECMA-402 12.1.6: format is
 * the concatenation of the parts. Deno and Bun return U+0020 from both and
 * satisfy the invariant, so the separator normalizes to U+0020 here and the
 * table stays self-consistent under the invariant the specification states.
 */
const quote = l => {
  const t = l.replace(/\u202f/g, ' ');
  return /^[^A-Za-z]*$/.test(t) ? t : "'" + t.replace(/'/g, "''") + "'";
};
function pattern(locale, opts, hc) {
  let f;
  try { f = new Intl.DateTimeFormat(locale, Object.assign({timeZone:'UTC'}, opts)); } catch { return null; }
  HINT = {month:opts.month, weekday:opts.weekday, era:opts.era};
  let out = '';
  for (const p of f.formatToParts(PROBE)) {
    if (p.type === 'literal' || p.type === 'unknown') { out += quote(p.value); continue; }
    const t = token(p.type, p.value, hc);
    if (!t) return null;
    out += t;
  }
  return out;
}
/*
 * Field names in long, short, and narrow width. February 1 2026 is a Sunday,
 * so seven consecutive days from it enumerate the weekday names in the order
 * Date.prototype.getDay reports.
 */
function names(locale, kind) {
  const at = (opts, type, t) => new Intl.DateTimeFormat(locale, Object.assign({timeZone:'UTC'}, opts))
    .formatToParts(t).find(p => p.type === type).value.replace(/\u202f/g, ' ');
  if (kind === 'month') return ['long','short','narrow'].map(w =>
    Array.from({length:12}, (_, i) => at({month:w}, 'month', Date.UTC(2026, i, 15))));
  if (kind === 'weekday') return ['long','short','narrow'].map(w =>
    Array.from({length:7}, (_, i) => at({weekday:w}, 'weekday', Date.UTC(2026, 1, 1 + i))));
  if (kind === 'era') return ['long','short','narrow'].map(w =>
    [Date.UTC(-1, 5, 15), Date.UTC(2026, 5, 15)].map(t => at({era:w, year:'numeric'}, 'era', t)));
  if (kind === 'dayPeriod') return [[0, 13].map(h =>
    at({hour:'numeric', hourCycle:'h12'}, 'dayPeriod', Date.UTC(2026, 0, 1, h)))];
  if (kind === 'zone') return ['short', 'long'].map(w =>
    at({timeZoneName:w}, 'timeZoneName', Date.UTC(2026, 0, 1)));
  return [];
}

const dsig = (w,e,y,m,d) => w + e*4 + y*16 + m*48 + d*288;
const tsig = (h,mi,s,c) => h + mi*3 + s*9 + c*27;

const locales = (process.argv[2] || 'en-US,en-GB').split(',');
const pool = [], poolIndex = new Map();
const intern = s => { if (s === null) return 0xFFFF; let i = poolIndex.get(s); if (i === undefined) { i = pool.length; pool.push(s); poolIndex.set(s, i); } return i; };
const out = [];
let exact = 0, total = 0;

for (const locale of locales) {
  NAMES = null;
  NAMES = { month: names(locale, 'month'), weekday: names(locale, 'weekday'),
    era: names(locale, 'era'), zone: names(locale, 'zone') };
  const dates = new Uint16Array(864).fill(0xFFFF), times = new Uint16Array(108).fill(0xFFFF);
  for (let w = 0; w < 4; w++) for (let e = 0; e < 4; e++) for (let y = 0; y < 3; y++)
  for (let m = 0; m < 6; m++) for (let d = 0; d < 3; d++) {
    if (!w && !e && !y && !m && !d) continue;
    const o = {};
    if (w) o.weekday = WEEKDAY[w]; if (e) o.era = ERA[e]; if (y) o.year = YEAR[y];
    if (m) o.month = MONTH[m]; if (d) o.day = DAY[d];
    const p = pattern(locale, o, null);
    total++; if (p !== null) exact++;
    dates[dsig(w,e,y,m,d)] = intern(p);
  }
  for (let h = 0; h < 3; h++) for (let mi = 0; mi < 3; mi++) for (let s = 0; s < 3; s++) {
    if (!h && !mi && !s) continue;
    const o = {};
    if (h) o.hour = NUM2[h]; if (mi) o.minute = NUM2[mi]; if (s) o.second = NUM2[s];
    for (let c = 0; c < (h ? 4 : 1); c++) {
      const opts = h ? Object.assign({hourCycle: CYCLES[c]}, o) : o;
      const p = pattern(locale, opts, h ? CYCLES[c] : null);
      total++; if (p !== null) exact++;
      times[tsig(h,mi,s,c)] = intern(p);
    }
  }
  // Style shorthands resolve to a pattern of their own rather than to
  // component widths: resolvedOptions reports dateStyle and timeStyle and no
  // components, and en-GB's short date and medium time disagree with the
  // widths a component request of the same shape would produce.
  const styles = new Uint16Array(100).fill(0xFFFF);
  const STYLE = [null, 'full', 'long', 'medium', 'short'];
  for (let ds = 0; ds < 5; ds++) for (let ts = 0; ts < 5; ts++) {
    if (!ds && !ts) continue;
    for (let c = 0; c < 4; c++) {
      const o = {};
      if (ds) o.dateStyle = STYLE[ds];
      if (ts) { o.timeStyle = STYLE[ts]; o.hourCycle = CYCLES[c]; }
      else if (c) continue;
      styles[(ds * 5 + ts) * 4 + c] = intern(pattern(locale, o, ts ? CYCLES[c] : null));
    }
  }

  // Glue joining a date pattern to a time pattern, selected by month width and
  // weekday presence the way CLDR selects dateTimeFormat by resolved style.
  const glue = [];
  for (let m = 0; m < 6; m++) for (let w = 0; w < 2; w++) {
    const o = {year:'numeric', day:'numeric', hour:'numeric', minute:'numeric', hourCycle:'h23'};
    if (m) o.month = MONTH[m]; if (w) o.weekday = 'short';
    const full = pattern(locale, o, 'h23');
    const dOpts = {year:'numeric', day:'numeric'};
    if (m) dOpts.month = MONTH[m]; if (w) dOpts.weekday = 'short';
    const dp = pattern(locale, dOpts, null);
    const tp = pattern(locale, {hour:'numeric', minute:'numeric', hourCycle:'h23'}, 'h23');
    glue.push(intern(full && dp && tp && full.startsWith(dp) && full.endsWith(tp)
      ? full.slice(dp.length, full.length - tp.length) : ', '));
  }
  const localNames = kind => {
    const f = w => new Intl.DateTimeFormat(locale, {timeZone:'UTC', month:w});
    if (kind === 'month') return ['long','short','narrow'].map(w =>
      Array.from({length:12}, (_, i) => f(w).formatToParts(Date.UTC(2026, i, 15)).find(p => p.type === 'month').value));
    if (kind === 'weekday') return ['long','short','narrow'].map(w =>
      Array.from({length:7}, (_, i) => new Intl.DateTimeFormat(locale, {timeZone:'UTC', weekday:w})
        .formatToParts(Date.UTC(2026, 1, 1 + i)).find(p => p.type === 'weekday').value)); // 2026-02-01 is a Sunday
    if (kind === 'era') return ['long','short','narrow'].map(w =>
      [Date.UTC(-1, 5, 15), Date.UTC(2026, 5, 15)].map(t =>
        new Intl.DateTimeFormat(locale, {timeZone:'UTC', era:w, year:'numeric'}).formatToParts(t).find(p => p.type === 'era').value));
    if (kind === 'dayPeriod') return [[0, 13].map(h =>
      new Intl.DateTimeFormat(locale, {timeZone:'UTC', hour:'numeric', hourCycle:'h12'})
        .formatToParts(Date.UTC(2026, 0, 1, h)).find(p => p.type === 'dayPeriod').value)];
    return [];
  };
  out.push({ locale, dates, times, glue, styles,
    months: NAMES.month, weekdays: NAMES.weekday, eras: NAMES.era,
    dayPeriods: names(locale, 'dayPeriod')[0], zoneNames: NAMES.zone,
    resolved: new Intl.DateTimeFormat(locale, {timeZone:'UTC'}).resolvedOptions() });
  console.error(`${locale}: pool now ${pool.length}`);
}
console.error(`patterns exact ${exact}/${total}`);

const esc = s => '"' + s.replace(/\u202f/g, ' ').replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
const arr = a => '[' + a.map(esc).join(', ') + ']';
const u16 = a => Array.from(a).map(v => v === 0xFFFF ? 'NONE' : String(v)).join(', ');
const wrap = (s, n) => { const o = []; let line = ''; for (const tok of s.split(', ')) { if (line.length + tok.length + 2 > n) { o.push(line); line = ''; } line += (line ? ', ' : '') + tok; } if (line) o.push(line); return o.map(l => '    ' + l + ',').join('\n'); };

let rs = `/*
 * Date and time formatting data for Intl.DateTimeFormat, generated by
 * scripts/gen_intl_datetime_data.mjs from the reference ECMA-402
 * implementation's own output; see docs/design/ARCHITECTURE-DECISIONS.md
 * AD-033 for why the table is enumerated rather than derived.
 *
 * PATTERNS holds every distinct pattern once, one per line. DATE_PATTERNS
 * indexes it by a dense signature over the five date field widths and
 * TIME_PATTERNS by a dense signature over the three time field widths and the
 * hour cycle, so a lookup is one array read and carries no matching step.
 *
 * The data derives from the Unicode Common Locale Data Repository and carries
 * the Unicode license recorded in NOTICE-CLDR.
 */

/// Signature slot no pattern occupies. Every combination an option bag can
/// reach carries a pattern; the empty slots are signatures no request
/// produces -- the empty field set, and the hour cycle variants of a time
/// signature that names no hour -- and formatting reports a reached one
/// rather than rendering an empty string.
pub(super) const NONE: u16 = u16::MAX;

/// Every distinct pattern, one per line, shared across locales.
pub(super) const PATTERNS: &str = "\\
${pool.map(p => p.replace(/\\/g, '\\\\')).join('\n')}";

/// Locale data for one supported locale.
pub(super) struct LocaleData {
    /// The canonical tag \`resolvedOptions().locale\` reports.
    pub(super) tag: &'static str,
    /// Date patterns indexed by \`date_signature\`.
    pub(super) dates: &'static [u16; 864],
    /// Time patterns indexed by \`time_signature\`.
    pub(super) times: &'static [u16; 108],
    /// Glue joining a date pattern to a time pattern, indexed by
    /// \`month_width * 2 + weekday_present\`.
    pub(super) glue: &'static [u16; 12],
    /// Patterns for the style shorthands, indexed by
    /// \`(date_style * 5 + time_style) * 4 + hour_cycle\`.
    pub(super) styles: &'static [u16; 100],
    /// Month names in long, short, and narrow width, January first.
    pub(super) months: [[&'static str; 12]; 3],
    /// Weekday names in long, short, and narrow width, Sunday first.
    pub(super) weekdays: [[&'static str; 7]; 3],
    /// Era names in long, short, and narrow width, BC then AD.
    pub(super) eras: [[&'static str; 2]; 3],
    /// The AM and PM day period names.
    pub(super) day_periods: [&'static str; 2],
    /// UTC's short and long names, which a style pattern's zone token renders
    /// when the request names UTC.
    pub(super) zone_names: [&'static str; 2],
    /// The hour cycle a request that names none resolves to.
    pub(super) hour_cycle: &'static str,
}

`;
for (const l of out) {
  const id = l.locale.replace(/-/g, '_').toUpperCase();
  rs += `static ${id}_DATES: [u16; 864] = [\n${wrap(u16(l.dates), 92)}\n];\n\n`;
  rs += `static ${id}_TIMES: [u16; 108] = [\n${wrap(u16(l.times), 92)}\n];\n\n`;
  rs += `static ${id}_GLUE: [u16; 12] = [${u16(l.glue)}];\n\n`;
  rs += `static ${id}_STYLES: [u16; 100] = [\n${wrap(u16(l.styles), 92)}\n];\n\n`;
}
rs += `/// Every locale this build formats. A request outside the set negotiates to\n/// the first entry, which \`resolvedOptions().locale\` then reports.\npub(super) static LOCALES: &[LocaleData] = &[\n`;
for (const l of out) {
  const id = l.locale.replace(/-/g, '_').toUpperCase();
  rs += `    LocaleData {
        tag: ${esc(l.locale)},
        dates: &${id}_DATES,
        times: &${id}_TIMES,
        glue: &${id}_GLUE,
        styles: &${id}_STYLES,
        months: [${l.months.map(arr).join(', ')}],
        weekdays: [${l.weekdays.map(arr).join(', ')}],
        eras: [${l.eras.map(arr).join(', ')}],
        day_periods: ${arr(l.dayPeriods)},
        zone_names: ${arr(l.zoneNames)},
        hour_cycle: ${esc(new Intl.DateTimeFormat(l.locale, {timeZone:'UTC', hour:'numeric'}).resolvedOptions().hourCycle)},
    },\n`;
}
rs += `];\n`;
process.stdout.write(rs);
