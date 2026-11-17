//! Steam Workshop browsing, item details and the Steam Web API.

use std::collections::HashMap;
use std::time::Duration;

use regex::Regex;
use reqwest::Client;
use serde_json::{json, Value};

pub const APP_ID: &str = "431960";
pub const WORKSHOP_BROWSE_URL: &str = "https://steamcommunity.com/workshop/browse/";
pub const WORKSHOP_ITEM_URL: &str = "https://steamcommunity.com/sharedfiles/filedetails/";
pub const STEAM_API_URL: &str =
    "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";

/// Steam sorts WallForge understands, mapped to Steam's browse parameters.
const SORT_PRESETS: &[(&str, &str, &str)] = &[
    ("top_rated", "toprated", "-1"),
    ("trend_year", "trend", "365"),
    ("trend_month", "trend", "30"),
    ("trend_week", "trend", "7"),
    ("trend_today", "trend", "1"),
    ("most_recent", "mostrecent", "-1"),
    ("most_up_votes", "toprated", "-1"),
    ("subscriptions", "trend", "365"),
];

pub fn resolve_sort(sort_method: &str, time_period: &str) -> (String, String) {
    if let Some((_, browse, days)) = SORT_PRESETS.iter().find(|(key, _, _)| *key == sort_method) {
        return ((*browse).to_string(), (*days).to_string());
    }
    ("trend".to_string(), time_period.to_string())
}

pub fn http_client() -> Result<Client, String> {
    Client::builder()
        .user_agent("Mozilla/5.0")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())
}

/// Steam wants a browser-like Accept-Language header.
pub fn with_language(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.header("Accept-Language", "en-US,en;q=0.9")
}

/// Strip the query string so the original asset is used.
pub fn normalise_image_url(url: &str) -> String {
    let url = url.trim();
    if url.starts_with("//") {
        return format!("https:{url}")
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
    }
    url.split('?').next().unwrap_or_default().to_string()
}

/// A Workshop author page off the item page, e.g. `…/profiles/7656…/`.
/// Vanity (`…/id/…`) pages work the same way.
pub fn valid_author_url(value: &str) -> bool {
    let lowered = value.trim().to_lowercase();
    lowered.starts_with("https://steamcommunity.com/profiles/")
        || lowered.starts_with("https://steamcommunity.com/id/")
}

pub fn valid_workshop_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.chars().all(|ch| ch.is_ascii_digit())
}

fn as_int(value: Option<&Value>, fallback: i64) -> i64 {
    match value.and_then(Value::as_i64) {
        Some(number) => number,
        None => match value.and_then(Value::as_str).and_then(|s| s.parse::<i64>().ok()) {
            Some(number) => number,
            None => fallback,
        },
    }
}

/// Steam's embedded SSR state (`<script id="valve-ssr-data">`).
fn load_ssr_payload(html: &str) -> Option<Value> {
    let pattern = Regex::new(r#"(?s)<script[^>]*id="valve-ssr-data"[^>]*>(.*?)</script>"#).ok()?;
    let captures = pattern.captures(html)?;
    serde_json::from_str(captures.get(1)?.as_str().trim()).ok()
}

/// Read a JSON value as text, whether it is a string or a number.
fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// Read a react-query entry (`queryKey[0] == key`) from the SSR payload.
fn query_data(payload: &Value, key: &str, app_id: Option<&str>) -> Option<Value> {
    let render_context = payload.get("renderContext")?;
    let mut query_data = render_context.get("queryData")?.clone();
    if let Some(text) = query_data.as_str() {
        query_data = serde_json::from_str(text).ok()?;
    }
    let queries = query_data.get("queries")?.as_array()?;
    for query in queries {
        let query_key = match query.get("queryKey").and_then(Value::as_array) {
            Some(list) if !list.is_empty() => list,
            _ => continue,
        };
        if query_key[0].as_str() != Some(key) {
            continue;
        }
        if let Some(app) = app_id {
            // The app id can be a number in the query key, so compare loosely.
            if query_key.len() < 2 || value_as_string(&query_key[1]) != app {
                continue;
            }
        }
        if let Some(data) = query.get("state").and_then(|state| state.get("data")) {
            if !data.is_null() {
                return Some(data.clone());
            }
        }
    }
    None
}

/// Locate the workshop_browse response inside the nested SSR payload.
fn find_browse_payload(node: &Value, depth: usize) -> Option<Value> {
    if depth > 8 {
        return None;
    }
    match node {
        Value::Object(map) => {
            if let Some(results) = map.get("results").and_then(Value::as_array) {
                if let Some(first) = results.first() {
                    if first.get("publishedfileid").is_some() {
                        return Some(node.clone());
                    }
                }
            }
            for value in map.values() {
                if let Some(found) = find_browse_payload(value, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(list) => {
            for value in list {
                if let Some(found) = find_browse_payload(value, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        Value::String(text) => {
            let trimmed = text.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                    return find_browse_payload(&parsed, depth + 1);
                }
            }
            None
        }
        _ => None,
    }
}

fn browse_data(payload: &Value) -> Option<Value> {
    if let Some(data) = query_data(payload, "workshop_browse", None) {
        if data.get("results").map(Value::is_array).unwrap_or(false) {
            return Some(data);
        }
    }
    find_browse_payload(payload, 0)
}

fn creator_names(browse: &Value) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let details = match browse
        .get("creator_player_link_details")
        .and_then(Value::as_array)
    {
        Some(list) => list,
        None => return names,
    };
    for entry in details {
        let public = match entry.get("public_data") {
            Some(value) => value,
            None => continue,
        };
        let steam_id = public
            .get("steamid")
            .map(|value| match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let persona = public
            .get("persona_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !steam_id.is_empty() && !persona.is_empty() {
            names.insert(steam_id, persona.to_string());
        }
    }
    names
}

/// Turn a browse payload into the items the interface shows.
pub fn items_from_browse(browse: &Value) -> Vec<Value> {
    let names = creator_names(browse);
    let results = match browse.get("results").and_then(Value::as_array) {
        Some(list) => list,
        None => return Vec::new(),
    };
    let mut items = Vec::new();
    for raw in results {
        let pfid = raw
            .get("publishedfileid")
            .map(|value| match value {
                Value::String(text) => text.trim().to_string(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        if !valid_workshop_id(&pfid) {
            continue;
        }
        let stars = raw
            .get("star_rating")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let creator = raw
            .get("creator")
            .map(|value| match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        items.push(json!({
            "id": pfid,
            "title": raw.get("title").and_then(Value::as_str).unwrap_or(&pfid).trim(),
            "img": normalise_image_url(raw.get("preview_url").and_then(Value::as_str).unwrap_or("")),
            "author": names.get(&creator).cloned().unwrap_or_default(),
            "link": format!("{WORKSHOP_ITEM_URL}?id={pfid}"),
            "rating": if (0..=5).contains(&stars) { "\u{2605}".repeat(stars as usize) } else { String::new() },
            "rating_img": "",
        }));
    }
    items
}

/// The old Workshop markup, used when the SSR payload is missing.
pub fn items_from_legacy_html(html: &str) -> Vec<Value> {
    let Ok(pattern) = Regex::new(r#"(?s)<div[^>]*class="workshopItem"[^>]*>(.*?)</div>\s*</div>"#) else {
        return Vec::new();
    };
    let blocks: Vec<String> = pattern
        .captures_iter(html)
        .map(|capture| capture[1].to_string())
        .collect();
    let mut items = Vec::new();
    for block in blocks {
        let id_pattern = Regex::new(r#"data-publishedfileid="(\d+)""#).ok();
        let pfid = id_pattern
            .as_ref()
            .and_then(|regex| regex.captures(&block))
            .map(|capture| capture[1].to_string())
            .unwrap_or_default();
        if !valid_workshop_id(&pfid) {
            continue;
        }
        let title_pattern = Regex::new(r#"(?s)<div class="workshopItemTitle[^"]*">(.*?)</div>"#).ok();
        let title = title_pattern
            .as_ref()
            .and_then(|regex| regex.captures(&block))
            .map(|capture| {
                html_escape::decode_html_entities(&strip_tags(&capture[1]))
                    .trim()
                    .to_string()
            })
            .unwrap_or_else(|| pfid.clone());
        let img_pattern = Regex::new(r#"class="workshopItemPreviewImage[^"]*"\s+src="([^"]+)""#).ok();
        let img = img_pattern
            .as_ref()
            .and_then(|regex| regex.captures(&block))
            .map(|capture| normalise_image_url(&capture[1]))
            .unwrap_or_default();
        let author_pattern = Regex::new(r#"(?s)<div class="workshopItemAuthorName">.*?>([^<>]+)<"#).ok();
        let author = author_pattern
            .as_ref()
            .and_then(|regex| regex.captures(&block))
            .map(|capture| capture[1].trim().to_string())
            .unwrap_or_default();
        items.push(json!({
            "id": pfid,
            "title": title,
            "img": img,
            "author": author,
            "link": format!("{WORKSHOP_ITEM_URL}?id={pfid}"),
            "rating": "",
            "rating_img": "",
        }));
    }
    items
}

fn strip_tags(html: &str) -> String {
    let tags = Regex::new(r"<[^>]*>").unwrap();
    tags.replace_all(html, "").to_string()
}

/// Browse the Workshop.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_top_wallpapers(
    client: &Client,
    page: i64,
    search_text: &str,
    sort_method: &str,
    time_period: &str,
    num_per_page: i64,
    required_tags: &[String],
    excluded_tags: &[String],
) -> Result<Value, String> {
    let page = page.max(1);
    let num_per_page = num_per_page.clamp(1, 30);
    let (browse_sort, days) = resolve_sort(sort_method, time_period);
    let search_text = search_text.trim();

    let mut params: Vec<(String, String)> = vec![
        ("appid".into(), APP_ID.into()),
        ("browsesort".into(), browse_sort.clone()),
        ("actualsort".into(), browse_sort),
        ("section".into(), "readytouseitems".into()),
        ("p".into(), page.to_string()),
        ("days".into(), days),
        ("numperpage".into(), num_per_page.to_string()),
        ("created_date_range_filter_start".into(), "0".into()),
        ("created_date_range_filter_end".into(), "0".into()),
        ("updated_date_range_filter_start".into(), "0".into()),
        ("updated_date_range_filter_end".into(), "0".into()),
    ];
    if !search_text.is_empty() {
        params.push(("searchtext".into(), search_text.to_string()));
    }
    for tag in required_tags {
        params.push(("requiredtags[]".into(), tag.clone()));
    }
    for tag in excluded_tags {
        params.push(("excludedtags[]".into(), tag.clone()));
    }

    let response = with_language(client.get(WORKSHOP_BROWSE_URL).query(&params))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let html = response.text().await.map_err(|err| err.to_string())?;

    match load_ssr_payload(&html).and_then(|payload| browse_data(&payload)) {
        Some(browse) => Ok(json!({
            "items": items_from_browse(&browse),
            "page": as_int(browse.get("current_page"), page),
            "total_pages": as_int(browse.get("total_pages"), 0),
            "total_count": as_int(browse.get("total_count"), 0),
        })),
        None => Ok(json!({
            "items": items_from_legacy_html(&html),
            "page": page,
            "total_pages": 0,
            "total_count": 0,
        })),
    }
}

/// The official filter groups, from Steam's SSR data.
pub async fn fetch_filter_groups(client: &Client) -> Result<Vec<Value>, String> {
    let response = with_language(
        client
            .get(WORKSHOP_BROWSE_URL)
            .query(&[("appid", APP_ID), ("section", "readytouseitems")]),
    )
    .send()
    .await
    .map_err(|err| err.to_string())?;
    let html = response.text().await.map_err(|err| err.to_string())?;
    let payload = match load_ssr_payload(&html) {
        Some(value) => value,
        None => return Ok(Vec::new()),
    };
    let raw = query_data(&payload, "declared_tags_v6", Some(APP_ID))
        .and_then(|data| data.get("readytouse_tags").cloned())
        .unwrap_or(Value::Null);
    Ok(normalise_filter_groups(&raw))
}

fn normalise_filter_groups(raw: &Value) -> Vec<Value> {
    let mut groups = Vec::new();
    let list = match raw.as_array() {
        Some(list) => list,
        None => return groups,
    };
    for group in list {
        let name = group
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let kind = if group.get("htmlelement").and_then(Value::as_str) == Some("checkbox") {
            "checkbox"
        } else {
            "select"
        };
        let mut tags: Vec<String> = Vec::new();
        if let Some(raw_tags) = group.get("tags").and_then(Value::as_array) {
            for tag in raw_tags {
                if tag.get("admin_only").and_then(Value::as_bool).unwrap_or(false) {
                    continue;
                }
                let display = tag
                    .get("display_name")
                    .and_then(Value::as_str)
                    .or_else(|| tag.get("name").and_then(Value::as_str))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !display.is_empty() && !tags.contains(&display) {
                    tags.push(display);
                }
            }
        }
        if !name.is_empty() && !tags.is_empty() {
            groups.push(json!({
                "name": name,
                "type": kind,
                "tags": tags,
            }));
        }
    }
    groups
}

/// Basic details for several items in one Steam Web API request.
pub async fn fetch_items_metadata(
    client: &Client,
    workshop_ids: &[String],
) -> Result<HashMap<String, Value>, String> {
    let mut ids: Vec<String> = Vec::new();
    for id in workshop_ids {
        let value = id.trim();
        if valid_workshop_id(value) && !ids.contains(&value.to_string()) {
            ids.push(value.to_string());
        }
    }
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut form: Vec<(String, String)> = vec![("itemcount".into(), ids.len().to_string())];
    for (index, id) in ids.iter().enumerate() {
        form.push((format!("publishedfileids[{}]", index), id.clone()));
    }

    let response = with_language(client.post(STEAM_API_URL).form(&form))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let payload: Value = response.json().await.map_err(|err| err.to_string())?;
    let details = payload
        .pointer("/response/publishedfiledetails")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut results = HashMap::new();
    for detail in details {
        if detail.get("result").and_then(Value::as_i64) != Some(1) {
            continue;
        }
        let id = detail
            .get("publishedfileid")
            .map(|value| match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let title = detail
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .trim()
            .to_string();
        let preview = detail
            .get("preview_url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let author = detail
            .get("consumer_app_id")
            .and_then(Value::as_i64)
            .map(|_| String::new())
            .unwrap_or_default();
        results.insert(
            id.clone(),
            json!({
                "id": id,
                "title": title,
                "author": author,
                "img": normalise_image_url(preview),
                "link": format!("{WORKSHOP_ITEM_URL}?id={id}"),
                "rating": "",
            }),
        );
    }
    Ok(results)
}

/// Full details for one Workshop item, read from its item page.
pub async fn fetch_item_details(client: &Client, workshop_id: &str) -> Result<Value, String> {
    let url = format!("{WORKSHOP_ITEM_URL}?id={workshop_id}");
    let response = with_language(client.get(&url))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let html = response.text().await.map_err(|err| err.to_string())?;
    parse_item_details(&html, workshop_id)
}

/// Read the item page into the fields the details panel shows.
pub fn parse_item_details(html: &str, workshop_id: &str) -> Result<Value, String> {
    let document = scraper::Html::parse_document(html);

    if select_one(&document, "#agegate").is_some() {
        return Err("This item is age restricted.".to_string());
    }

    let title = select_one(&document, ".workshopItemTitle")
        .map(|element| element_text(element))
        .unwrap_or_else(|| workshop_id.to_string());

    let tag_blocks = select_all(&document, ".workshopTags");
    if title == workshop_id && tag_blocks.is_empty() {
        return Err("The Workshop page could not be read.".to_string());
    }

    let mut author = "Unknown".to_string();
    let mut author_url = String::new();
    if let Some(element) = select_one(&document, ".friendBlockContent") {
        let text = element_text(element);
        if !text.is_empty() {
            author = text;
        }
    }
    if let Some(link) = select_one(&document, ".friendBlockLinkOverlay") {
        if let Some(href) = link.value().attr("href") {
            author_url = href.to_string();
        }
    }

    let img = select_one(&document, "#previewImageMain")
        .or_else(|| select_one(&document, "#previewImage"))
        .or_else(|| select_one(&document, ".workshopItemPreviewImageMain"))
        .and_then(|element| element.value().attr("src").map(normalise_image_url))
        .unwrap_or_default();

    let images = detail_preview_images(html, &document, &img);

    let mut video_id = String::new();
    if let Ok(regex) = Regex::new(r#"YOUTUBE_VIDEO_ID\s*:\s*["']([\w-]{6,})["']"#) {
        if let Some(captures) = regex.captures(html) {
            video_id = captures[1].to_string();
        }
    }

    let mut stats: HashMap<String, String> = HashMap::new();
    let lefts: Vec<String> = select_all(&document, ".detailsStatLeft")
        .iter()
        .map(|element| {
            element_text(*element)
                .trim()
                .trim_end_matches(':')
                .trim()
                .to_lowercase()
        })
        .collect();
    let rights: Vec<String> = select_all(&document, ".detailsStatRight")
        .iter()
        .map(|element| element_text(*element))
        .collect();
    for (label, value) in lefts.iter().zip(rights.iter()) {
        if !label.is_empty() && !value.is_empty() {
            stats.insert(label.clone(), value.clone());
        }
    }

    let mut stars = String::new();
    if let Some(element) = select_one(&document, ".fileRatingDetails") {
        if let Some(src) = element.value().attr("src") {
            stars = rating_image_to_stars(src);
        }
    }
    let rating_count = select_one(&document, ".numRatings")
        .map(|element| element_text(element))
        .unwrap_or_default();

    let mut tags: Vec<Value> = Vec::new();
    for block in &tag_blocks {
        let group = select_one_in(*block, ".workshopTagsTitle")
            .map(|element| {
                element_text(element)
                    .trim()
                    .trim_end_matches(':')
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        let name = select_one_in(*block, "a")
            .map(|element| element_text(element))
            .or_else(|| Some(element_text(*block)))
            .unwrap_or_default();
        if !name.is_empty() {
            tags.push(json!({"group": group, "name": name}));
        }
    }

    let description = select_one(&document, ".workshopItemDescription")
        .map(|element| truncate_description(&element_text(element)))
        .unwrap_or_default();

    Ok(json!({
        "id": workshop_id,
        "title": title,
        "author": author,
        "author_url": author_url,
        "img": img,
        "images": images,
        "video_id": video_id,
        "link": format!("{WORKSHOP_ITEM_URL}?id={workshop_id}"),
        "rating": stars,
        "rating_count": rating_count,
        "file_size": stats.get("file size").cloned().unwrap_or_default(),
        "posted": stats.get("posted").cloned().unwrap_or_default(),
        "updated": stats.get("updated").cloned().unwrap_or_default(),
        "tags": tags,
        "description": description,
    }))
}

/// Run a CSS selector and return the first match.
fn select_one<'a>(
    root: &'a scraper::Html,
    selector: &str,
) -> Option<scraper::ElementRef<'a>> {
    select_one_in(root.root_element(), selector)
}

/// Run a CSS selector and return the first match inside an element.
fn select_one_in<'a>(
    root: scraper::ElementRef<'a>,
    selector: &str,
) -> Option<scraper::ElementRef<'a>> {
    let parsed = scraper::Selector::parse(selector).ok()?;
    root.select(&parsed).next()
}

/// Run a CSS selector and return every match.
fn select_all<'a>(root: &'a scraper::Html, selector: &str) -> Vec<scraper::ElementRef<'a>> {
    let Ok(parsed) = scraper::Selector::parse(selector) else {
        return Vec::new();
    };
    root.root_element().select(&parsed).collect()
}

/// The text of an element, with the whitespace collapsed.
fn element_text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Full-size screenshots first, then the main preview as the last resort.
fn detail_preview_images(html: &str, document: &scraper::Html, main_img: &str) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(regex) = Regex::new(r"(?s)var\s+rgFullScreenshotURLs\s*=\s*(\[.*?\]);") {
        if let Some(captures) = regex.captures(html) {
            if let Ok(url_regex) = Regex::new(r#"['"]url['"]\s*:\s*['"]([^'"]+)['"]"#) {
                for url in url_regex.captures_iter(&captures[1]) {
                    candidates.push(url[1].to_string());
                }
            }
        }
    }
    for element in select_all(document, ".workshopItemPreviewImage") {
        if let Some(src) = element.value().attr("src") {
            candidates.push(src.to_string());
        }
    }

    let mut images: Vec<String> = Vec::new();
    for raw in candidates {
        let url = normalise_image_url(&raw);
        if !url.is_empty() && url != main_img && !images.contains(&url) {
            images.push(url);
        }
    }
    if !main_img.is_empty() {
        images.push(main_img.to_string());
    }
    images
}

/// Steam's rating image carries the star count in its file name.
pub fn rating_image_to_stars(src: &str) -> String {
    let pattern = Regex::new(r"(\d+)-star|star_(\d+)|(\d)star").ok();
    let stars = pattern
        .and_then(|regex| regex.captures(src))
        .and_then(|captures| {
            captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))
                .and_then(|value| value.as_str().parse::<usize>().ok())
        })
        .unwrap_or(0);
    if stars == 0 || stars > 5 {
        String::new()
    } else {
        "\u{2605}".repeat(stars)
    }
}

fn truncate_description(text: &str) -> String {
    let limit = 4000;
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_urls_are_validated() {
        assert!(valid_author_url("https://steamcommunity.com/profiles/76561199028194439"));
        assert!(valid_author_url("https://steamcommunity.com/id/someone/"));
        assert!(!valid_author_url("https://example.com/profiles/76561199028194439"));
        assert!(!valid_author_url("javascript:alert(1)"));
        assert!(!valid_author_url("file:///C:/Windows"));
        assert!(!valid_author_url("https://steamcommunity.com.evil.com/profiles/1"));
    }

    #[test]
    fn image_urls_lose_the_query() {
        assert_eq!(
            normalise_image_url("https://cdn.example/a.jpg?imw=637"),
            "https://cdn.example/a.jpg"
        );
        assert_eq!(
            normalise_image_url("//cdn.example/a.jpg?x=1"),
            "https://cdn.example/a.jpg"
        );
    }

    #[test]
    fn sorts_map_to_steam_parameters() {
        assert_eq!(
            resolve_sort("top_rated", "-1"),
            ("toprated".to_string(), "-1".to_string())
        );
        assert_eq!(
            resolve_sort("trend_month", "-1"),
            ("trend".to_string(), "30".to_string())
        );
    }

    #[test]
    fn rating_images_become_stars() {
        assert_eq!(rating_image_to_stars("https://x/5-star_large.png"), "\u{2605}\u{2605}\u{2605}\u{2605}\u{2605}");
        assert_eq!(rating_image_to_stars("https://x/nope.png"), "");
    }

    #[test]
    fn browse_payload_becomes_items() {
        let browse = json!({
            "results": [{
                "publishedfileid": "123456",
                "title": "A wallpaper",
                "preview_url": "https://cdn.example/a.jpg?imw=268",
                "creator": "7656119",
                "star_rating": 5
            }],
            "creator_player_link_details": [{
                "public_data": {"steamid": "7656119", "persona_name": "Someone"}
            }]
        });
        let items = items_from_browse(&browse);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["author"], "Someone");
        assert_eq!(items[0]["img"], "https://cdn.example/a.jpg");
    }

    #[test]
    fn details_prefer_the_full_screenshot() {
        let html = r##"
        <html><body>
          <h1 class="workshopItemTitle">Neon Alley</h1>
          <img id="previewImage" src="https://cdn.example/neon.jpg?imw=637">
          <script>
            var rgFullScreenshotURLs = [
              { 'url': 'https://cdn.example/big.jpg?imw=5000' },
              { 'url': 'https://cdn.example/neon.jpg?imw=5000' },
            ];
          </script>
          <div class="workshopTags"><span class="workshopTagsTitle">Type:</span><a href="#">Scene</a></div>
        </body></html>"##;
        let details = parse_item_details(html, "888").expect("details must parse");
        assert_eq!(details["title"], "Neon Alley");
        assert_eq!(
            details["images"],
            json!(["https://cdn.example/big.jpg", "https://cdn.example/neon.jpg"])
        );
    }
}

