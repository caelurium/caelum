use super::Report;
use caelum_protocol::*;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub(super) fn read(
    db: &Connection,
    filter: &OpenCodeStatsFilter,
    server: bool,
) -> rusqlite::Result<Report> {
    let pred="FROM turns WHERE timestamp>=?1 AND timestamp<?2 AND (?3 IS NULL OR project_path=?3) AND (?4 IS NULL OR provider=?4)";
    let included = if server {
        format!("{pred} AND provider!='OpenCode'")
    } else {
        pred.to_owned()
    };
    let bind = params![
        filter.from.unwrap_or(0),
        filter.to.unwrap_or(i64::MAX as u64),
        filter.project,
        filter.provider.map(StatsProvider::name)
    ];
    let (first, last): (Option<u64>, Option<u64>) = db.query_row(
        &format!("SELECT min(timestamp),max(timestamp) {included}"),
        bind,
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut stats = OpenCodeStats {
        duration_ms: db.query_row(
            &format!("SELECT coalesce(sum(duration_ms),0) {pred}"),
            bind,
            |r| r.get(0),
        )?,
        sessions: db.query_row(
            &format!(
                "SELECT count(*) FROM (SELECT DISTINCT provider,project_path,session_id {included})"
            ),
            bind,
            |r| r.get(0),
        )?,
        range: StatsRange {
            from: filter.from.or(first).unwrap_or_default() as f64,
            to: filter.to.or(last).unwrap_or_default() as f64,
        },
        ..Default::default()
    };
    let mut groups=db.prepare(&format!("SELECT provider,model,count(*),sum(input_tokens),sum(output_tokens),sum(reasoning_tokens),sum(cached_tokens),sum(cache_write_tokens),coalesce(sum(estimated_cost_usd),0),sum(estimated_cost_usd IS NULL),sum(estimated_tokens) {included} GROUP BY provider,model ORDER BY count(*) DESC,provider,model"))?;
    let mut rows = groups.query(bind)?;
    while let Some(r) = rows.next()? {
        let m = StatsModelUsage {
            model: StatsModelRef {
                provider_id: r.get(0)?,
                id: r.get(1)?,
                variant: None,
            },
            steps: r.get(2)?,
            tokens: StatsTokens {
                input: r.get(3)?,
                output: r.get(4)?,
                reasoning: r.get(5)?,
                cache: StatsCacheTokens {
                    read: r.get(6)?,
                    write: r.get(7)?,
                },
            },
            cost: r.get(8)?,
            unpriced: r.get(9)?,
        };
        stats.prompts += m.steps;
        stats.steps += m.steps;
        stats.cost += m.cost;
        stats.unpriced += m.unpriced;
        stats.estimated_turns += r.get::<_, u64>(10)?;
        add_tokens(&mut stats.tokens, &m.tokens);
        stats.models.push(m);
    }
    let mut days=db.prepare(&format!("SELECT date(timestamp/1000,'unixepoch') AS day,count(*) {included} GROUP BY day ORDER BY day"))?;
    stats.activity = days
        .query_map(bind, |r| {
            Ok(StatsActivity {
                date: r.get(0)?,
                steps: r.get(1)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    activity(&mut stats);
    let mut projects =
        db.prepare("SELECT DISTINCT project_path FROM turns ORDER BY project_path")?;
    let projects = projects
        .query_map([], |r| {
            let path: String = r.get(0)?;
            let name = std::path::Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned);
            Ok(OpenCodeStatsProject {
                id: path.clone(),
                canonical: path,
                name,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok((stats, projects))
}

fn add_tokens(a: &mut StatsTokens, b: &StatsTokens) {
    a.input += b.input;
    a.output += b.output;
    a.reasoning += b.reasoning;
    a.cache.read += b.cache.read;
    a.cache.write += b.cache.write;
}

pub(super) fn merge(local: &mut OpenCodeStats, server: OpenCodeStats) {
    if local.prompts == 0 {
        let (duration, dropped) = (local.duration_ms, local.dropped);
        *local = server;
        local.duration_ms = duration;
        local.dropped = dropped;
        return;
    } else if server.steps > 0 {
        local.range.from = local.range.from.min(server.range.from);
        local.range.to = local.range.to.max(server.range.to);
    }
    local.prompts += server.prompts;
    local.steps += server.steps;
    local.sessions += server.sessions;
    local.subagents += server.subagents;
    local.cost += server.cost;
    add_tokens(&mut local.tokens, &server.tokens);
    local.models.extend(server.models);
    local.models.sort_by_key(|m| std::cmp::Reverse(m.steps));
    let mut days = BTreeMap::new();
    for d in local.activity.drain(..).chain(server.activity) {
        *days.entry(d.date).or_insert(0) += d.steps;
    }
    local.activity = days
        .into_iter()
        .map(|(date, steps)| StatsActivity { date, steps })
        .collect();
    activity(local);
}

fn activity(stats: &mut OpenCodeStats) {
    let mut last = None;
    stats.streak = 0;
    stats.active_days = 0;
    for d in stats.activity.iter().filter(|d| d.steps > 0) {
        stats.active_days += 1;
        let date = time::Date::parse(
            &d.date,
            &time::format_description::well_known::Iso8601::DATE,
        )
        .ok();
        stats.streak = if last.is_some() && date.and_then(|d| d.previous_day()) == last {
            stats.streak + 1
        } else {
            1
        };
        last = date;
    }
}
