// 只保存 Cloudflare 提供的国家／地区代码，不读取或保存原始 IP。
// ISO 3166-1 两位代码及 Cloudflare 可能使用的 XK；未知和特殊出口不推断地区。
const COUNTRY_CODES = new Set(`
  AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ
  BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ
  CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ
  DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR
  GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY
  HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP
  KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY
  MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ
  NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY
  QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ
  TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ
  VA VC VE VG VI VN VU WF WS XK YE YT ZA ZM ZW
`.trim().split(/\s+/));

export async function saveDailyCountry(request, env, anonymousId, usageDate, observedAt) {
  if (env.USAGE_COUNTRY_ENABLED !== "true" || usageDate !== observedAt.slice(0, 10)) return;
  const value = request.cf?.country;
  const country = typeof value === "string" ? value.toUpperCase() : null;
  if (!COUNTRY_CODES.has(country)) return;
  try {
    await env.DB.prepare(`
      INSERT INTO daily_usage_country(anonymous_id, observed_date, country_code, observed_at)
      VALUES (?, ?, ?, ?)
      ON CONFLICT(anonymous_id, observed_date) DO NOTHING
    `).bind(anonymousId, usageDate, country, observedAt).run();
  } catch {
    // 原计数已经成功；附属观察失败不能令客户端重试整个快照或泄漏请求详情。
    console.error("地区统计保存失败");
  }
}

export async function cleanupDailyCountries(env) {
  await env.DB.prepare(`
    DELETE FROM daily_usage_country WHERE observed_date < date('now', '-89 days')
  `).run();
}

