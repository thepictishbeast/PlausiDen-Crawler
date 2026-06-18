//! `video_captions_missing` — flags `<video>` elements without
//! a `<track kind="captions">` (or `kind="subtitles"` as a
//! weaker fallback).
//!
//! WCAG 2.1 SC 1.2.2 ("Captions (Prerecorded)", Level A): all
//! prerecorded synchronized media must have captions, except
//! when the video is itself a media alternative for text +
//! clearly labeled as such. The success criterion applies to
//! the prevailing recorded-video pattern — autoplay product
//! demos, embedded testimonial videos, course chapter videos,
//! tutorial recordings.
//!
//! Defect class: operator drops a `<video src="...">` (or
//! `<video><source>`) without a sibling `<track kind="captions"
//! src="..." srclang="...">`. Deaf + hard-of-hearing users
//! get no information from the audio track; lower-bandwidth +
//! noisy-environment users (the actual majority of caption
//! consumers in studies) get nothing either.
//!
//! Skip cases that don't carry the burden:
//!
//! * `<video muted>` AND no `<audio>` channel anywhere — pure
//!   visual loop / cinemagraph; captions are a no-op.
//! * `<video>` with `data-decorative="true"` opt-out (e.g.
//!   ambient hero loop the operator has declared decorative).
//! * `<video>` whose `aria-hidden="true"` removes it from the
//!   accessibility tree.
//!
//! ## Heuristic
//!
//! JS walks every `<video>` not in the skip-list. Captures:
//!
//! * `has_track_captions` — at least one `<track kind="captions">`
//!   child.
//! * `has_track_subtitles` — at least one `<track kind="subtitles">`
//!   child. Subtitles ≠ captions per WCAG (subtitles assume the
//!   user can hear; captions transcribe audio cues too). But
//!   subtitles still help most users so we treat the gap as
//!   Warn rather than Strict.
//! * `is_muted` — `muted` attribute present.
//!
//! ## Severity
//!
//! * **Strict** — no track AT ALL (no captions, no subtitles).
//!   Fails WCAG 1.2.2 outright.
//! * **Warn** — only subtitles, no captions. Closer to
//!   compliance but doesn't yet cover audio cues; flag for
//!   audit.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<video>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VideoCaptionsMissingHit {
    /// CSS-ish path of the offending `<video>`.
    pub selector: String,
    /// `src` attribute (empty if `<video>` uses `<source>`).
    pub src: String,
    /// Best-effort accessible name — `aria-label` or `title`,
    /// capped 60 chars. Empty when none resolved.
    pub label: String,
    /// True iff a `<track kind="captions">` child is present.
    pub has_track_captions: bool,
    /// True iff a `<track kind="subtitles">` child is present.
    pub has_track_subtitles: bool,
    /// True iff the `muted` attribute is set.
    pub is_muted: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VideoCaptionsMissingSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending `<video>` elements.
    pub hits: Vec<VideoCaptionsMissingHit>,
    /// Total `<video>` elements walked.
    pub scanned_videos: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_video_captions_missing(snap: &VideoCaptionsMissingSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut no_track: Vec<&VideoCaptionsMissingHit> = Vec::new();
    let mut subtitles_only: Vec<&VideoCaptionsMissingHit> = Vec::new();
    for h in &snap.hits {
        if h.has_track_captions {
            continue;
        }
        if h.has_track_subtitles {
            subtitles_only.push(h);
        } else {
            no_track.push(h);
        }
    }

    let format_example = |h: &VideoCaptionsMissingHit| -> String {
        let label = if h.label.is_empty() {
            String::new()
        } else {
            format!(" [{}]", h.label)
        };
        let src = if h.src.is_empty() {
            "<no src — uses <source> children>".to_owned()
        } else {
            format!("src=`{}`", h.src)
        };
        let muted = if h.is_muted { " · muted" } else { "" };
        format!("{}{} ({}{})", h.selector, label, src, muted)
    };

    let mut out = Vec::new();
    if !no_track.is_empty() {
        let examples: Vec<String> = no_track
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "video-captions.missing-all-tracks".to_owned(),
            detail: format!(
                "{} <video> element(s) with no <track> child at all — fails WCAG 1.2.2. Add `<track kind=\"captions\" src=\"…\" srclang=\"…\">`. Opt out per-element with `data-decorative=\"true\"` for ambient loops without audio. Examples: {}",
                no_track.len(),
                examples.join("; ")
            ),
        });
    }
    if !subtitles_only.is_empty() {
        let examples: Vec<String> = subtitles_only
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "video-captions.subtitles-only".to_owned(),
            detail: format!(
                "{} <video> element(s) carry `<track kind=\"subtitles\">` but no `<track kind=\"captions\">` — subtitles assume the user can hear; captions transcribe audio cues too. Add a captions track for WCAG 1.2.2 compliance. Examples: {}",
                subtitles_only.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<video>`,
/// captures track-kind presence + muted state.
///
/// Mirror any change in this file's `VideoCaptionsMissingHit`
/// + snapshot fields.
pub const VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    const labelOf = function(v) {
      const aria = v.getAttribute && v.getAttribute('aria-label');
      if (aria) return aria.trim().substring(0, 60);
      const title = v.getAttribute && v.getAttribute('title');
      if (title) return title.trim().substring(0, 60);
      return '';
    };

    const hits = [];
    let scanned = 0;
    const videos = document.querySelectorAll('video');
    for (const v of videos) {
      // Skip decorative-opted-out + aria-hidden videos.
      if (v.getAttribute && v.getAttribute('data-decorative') === 'true') continue;
      if (v.getAttribute && v.getAttribute('aria-hidden') === 'true') continue;
      scanned += 1;
      const muted = v.hasAttribute && v.hasAttribute('muted');
      // A muted video without an audio channel is effectively
      // decorative. Browsers don't expose audioTracks reliably
      // pre-load, so we use `muted` as the proxy + lean toward
      // flagging (assume the operator might enable audio
      // programmatically). If they don't, the opt-out attribute
      // is the right escape.
      const captionTracks = v.querySelectorAll(':scope > track[kind="captions"]');
      const subtitleTracks = v.querySelectorAll(':scope > track[kind="subtitles"]');
      const hasCaptions = captionTracks.length > 0;
      const hasSubtitles = subtitleTracks.length > 0;
      if (hasCaptions) continue;
      hits.push({
        selector: selectorOf(v),
        src: (v.getAttribute('src') || '').trim(),
        label: labelOf(v),
        hasTrackCaptions: hasCaptions,
        hasTrackSubtitles: hasSubtitles,
        isMuted: muted
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedVideos: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        src: &str,
        has_captions: bool,
        has_subtitles: bool,
        is_muted: bool,
    ) -> VideoCaptionsMissingHit {
        VideoCaptionsMissingHit {
            selector: selector.into(),
            src: src.into(),
            label: String::new(),
            has_track_captions: has_captions,
            has_track_subtitles: has_subtitles,
            is_muted,
        }
    }

    fn snap(hits: Vec<VideoCaptionsMissingHit>) -> VideoCaptionsMissingSnapshot {
        VideoCaptionsMissingSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_videos: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_video_captions_missing(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn video_with_captions_track_is_skipped_silently() {
        // Snapshot wouldn't normally include videos with captions
        // (JS pre-filters), but the detector must defensively
        // skip them too — never emit a finding when captions
        // are present.
        let s = snap(vec![hit(".hero", "/h.mp4", true, false, false)]);
        let findings = detect_video_captions_missing(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_track_at_all_is_strict() {
        let s = snap(vec![hit(".demo", "/d.mp4", false, false, false)]);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "video-captions.missing-all-tracks");
        assert!(findings[0].detail.contains(".demo"));
        assert!(findings[0].detail.contains("WCAG 1.2.2"));
    }

    #[test]
    fn subtitles_only_is_warn() {
        let s = snap(vec![hit(".tutorial", "/t.mp4", false, true, false)]);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "video-captions.subtitles-only");
        assert!(findings[0].detail.contains("subtitles assume the user can hear"));
    }

    #[test]
    fn mixed_emits_two_findings() {
        let s = snap(vec![
            hit(".a", "/a.mp4", false, false, false),
            hit(".b", "/b.mp4", false, true, false),
        ]);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"video-captions.missing-all-tracks"));
        assert!(kinds.contains(&"video-captions.subtitles-only"));
    }

    #[test]
    fn muted_marker_appears_in_example_detail() {
        let s = snap(vec![hit(".ambient", "/loop.mp4", false, false, true)]);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("muted"));
    }

    #[test]
    fn empty_src_uses_source_child_label() {
        let s = snap(vec![hit(".multi", "", false, false, false)]);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("<no src"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit(".testimonial", "/t.mp4", false, false, false);
        h.label = "Customer story: Acme Corp".into();
        let s = snap(vec![h]);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Customer story: Acme Corp]"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".v-{i}"), "/x.mp4", false, false, false));
        }
        let s = snap(hits);
        let findings = detect_video_captions_missing(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <video> element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("hits"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("scannedVideos"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("hasTrackCaptions"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("hasTrackSubtitles"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("isMuted"));
        // Selector contract — video element + scoped track query.
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("'video'"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains(":scope > track[kind=\"captions\"]"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains(":scope > track[kind=\"subtitles\"]"));
        // Opt-out contracts.
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("data-decorative"));
        assert!(VIDEO_CAPTIONS_MISSING_DOM_CAPTURE_JS.contains("aria-hidden"));
    }
}
