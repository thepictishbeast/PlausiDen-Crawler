//! `autoplay_media` — auto-playing audio + video audit.
//!
//! Per WCAG 2.1 Success Criterion 1.4.2 (Audio Control, A): "if
//! any audio on a Web page plays automatically for more than 3
//! seconds, either a mechanism is available to pause or stop the
//! audio, or a mechanism is available to control audio volume
//! independently from the overall system volume level."
//!
//! Beyond accessibility, autoplay also impacts:
//!   * privacy — auto-play with audio leaks the user's presence
//!     to bystanders
//!   * performance — auto-fetching a video on every page load
//!     burns bandwidth + the SWD v4 carbon budget
//!   * UX — autoplay is the canonical user-hostile pattern
//!
//! Findings:
//!   * `autoplay.unmuted`       strict   <video|audio> autoplay
//!                                        without muted/loop=false
//!   * `autoplay.no-controls`   strict   autoplay without controls=
//!                                        AND no muted attribute
//!   * `autoplay.long-loop`     warn     auto-playing loop > 30s
//!                                        (user has no way to stop)
//!   * `autoplay.no-poster`     warn     auto-playing video without
//!                                        poster= (CLS risk +
//!                                        forces buffer)
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Long-loop threshold beyond which a missing `controls=`
/// attribute fires a warn.
pub const LONG_LOOP_SECS: f64 = 30.0;

/// One captured media element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MediaEntry {
    /// CSS selector pointing at the `<audio>` / `<video>`.
    pub selector: String,
    /// `"audio"` or `"video"`.
    pub tag: String,
    /// `src` value or empty.
    pub src: String,
    /// Whether `autoplay` attribute is present.
    pub autoplay: bool,
    /// Whether `muted` attribute is present.
    pub muted: bool,
    /// Whether `controls` attribute is present.
    pub controls: bool,
    /// Whether `loop` attribute is present.
    pub looped: bool,
    /// Whether `poster=` attribute is set (video only).
    pub has_poster: bool,
    /// Duration in seconds if known (0 if not loaded).
    pub duration_secs: f64,
}

/// Captured media set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AutoplayMediaSnapshot {
    /// Page URL.
    pub page_url: String,
    /// All `<audio>` + `<video>` elements.
    pub media: Vec<MediaEntry>,
}

/// Page-side eval.
pub const AUTOPLAY_MEDIA_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
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

    const out = [];
    const els = document.querySelectorAll('audio, video');
    for (let i = 0; i < els.length; i++) {
        const el = els[i];
        const tag = el.tagName.toLowerCase();
        out.push({
            selector: selectorOf(el),
            tag: tag,
            src: el.getAttribute('src') || '',
            autoplay: el.hasAttribute('autoplay'),
            muted: el.hasAttribute('muted') || el.muted === true,
            controls: el.hasAttribute('controls'),
            looped: el.hasAttribute('loop'),
            hasPoster: tag === 'video' && el.hasAttribute('poster'),
            durationSecs: isFinite(el.duration) ? el.duration : 0
        });
    }
    return { pageUrl: window.location.href, media: out };
})()"##;

/// Run the detector.
pub fn detect_autoplay_media_issues(snap: &AutoplayMediaSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    for m in &snap.media {
        if !m.autoplay {
            continue;
        }
        // Unmuted autoplay is a WCAG 1.4.2 strict fail.
        if !m.muted {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "autoplay.unmuted".into(),
                detail: format!(
                    "<{} src=\"{}\"> autoplays unmuted; WCAG SC 1.4.2 ({})",
                    m.tag, m.src, m.selector
                ),
            });
        }
        // Autoplay without controls AND without muted is doubly bad —
        // user can't stop it.
        if !m.controls && !m.muted {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "autoplay.no-controls".into(),
                detail: format!(
                    "<{} src=\"{}\"> autoplays with no controls and no muted attribute ({})",
                    m.tag, m.src, m.selector
                ),
            });
        }
        // Looping autoplay over a long duration with no controls is
        // a warn — bandwidth/carbon cost climbs without user recourse.
        if m.looped && m.duration_secs > LONG_LOOP_SECS && !m.controls {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "autoplay.long-loop".into(),
                detail: format!(
                    "<{} src=\"{}\"> auto-loops {:.1}s with no controls (>{}s threshold) ({})",
                    m.tag, m.src, m.duration_secs, LONG_LOOP_SECS, m.selector
                ),
            });
        }
        // Auto-playing video without poster causes CLS + forces
        // buffer fetch before paint.
        if m.tag == "video" && !m.has_poster {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "autoplay.no-poster".into(),
                detail: format!(
                    "auto-playing <video src=\"{}\"> has no poster attribute; CLS risk + cold buffer ({})",
                    m.src, m.selector
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(
        tag: &str,
        autoplay: bool,
        muted: bool,
        controls: bool,
        looped: bool,
        poster: bool,
        dur: f64,
    ) -> MediaEntry {
        MediaEntry {
            selector: format!("{}[autoplay={}]", tag, autoplay),
            tag: tag.to_string(),
            src: format!("/{}.mp4", tag),
            autoplay,
            muted,
            controls,
            looped,
            has_poster: poster,
            duration_secs: dur,
        }
    }

    fn snap(media_list: Vec<MediaEntry>) -> AutoplayMediaSnapshot {
        AutoplayMediaSnapshot {
            page_url: "https://example.com/".into(),
            media: media_list,
        }
    }

    #[test]
    fn no_autoplay_is_clean() {
        let s = snap(vec![media("video", false, false, false, false, false, 0.0)]);
        assert!(detect_autoplay_media_issues(&s).is_empty());
    }

    #[test]
    fn unmuted_autoplay_is_strict() {
        let s = snap(vec![media("video", true, false, true, false, true, 10.0)]);
        let f = detect_autoplay_media_issues(&s);
        assert!(f.iter().any(|x| x.kind == "autoplay.unmuted"));
        assert!(f.iter().any(|x| x.severity == AxisSeverity::Strict));
    }

    #[test]
    fn autoplay_no_controls_no_mute_double_strict() {
        let s = snap(vec![media("audio", true, false, false, false, false, 5.0)]);
        let f = detect_autoplay_media_issues(&s);
        // Fires BOTH autoplay.unmuted AND autoplay.no-controls.
        assert!(f.iter().any(|x| x.kind == "autoplay.unmuted"));
        assert!(f.iter().any(|x| x.kind == "autoplay.no-controls"));
    }

    #[test]
    fn muted_autoplay_with_controls_is_clean_for_video_with_poster() {
        let s = snap(vec![media("video", true, true, true, false, true, 10.0)]);
        assert!(detect_autoplay_media_issues(&s).is_empty());
    }

    #[test]
    fn long_loop_autoplay_warns() {
        let s = snap(vec![media("video", true, true, false, true, true, 60.0)]);
        let f = detect_autoplay_media_issues(&s);
        assert!(f.iter().any(|x| x.kind == "autoplay.long-loop"));
    }

    #[test]
    fn short_loop_does_not_warn_long_loop() {
        let s = snap(vec![media("video", true, true, false, true, true, 10.0)]);
        let f = detect_autoplay_media_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "autoplay.long-loop"));
    }

    #[test]
    fn autoplay_video_without_poster_warns() {
        let s = snap(vec![media("video", true, true, true, false, false, 10.0)]);
        let f = detect_autoplay_media_issues(&s);
        assert!(f.iter().any(|x| x.kind == "autoplay.no-poster"));
    }

    #[test]
    fn autoplay_audio_without_poster_does_not_warn_poster() {
        // <audio> has no poster concept.
        let s = snap(vec![media("audio", true, true, true, false, false, 10.0)]);
        let f = detect_autoplay_media_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "autoplay.no-poster"));
    }
}
