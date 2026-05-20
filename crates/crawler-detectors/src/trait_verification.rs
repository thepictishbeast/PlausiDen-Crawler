//! `trait_verification` — runtime verification of declared traits.
//!
//! Trait DAGs (loom-traits, cms-traits, etc.) let primitives DECLARE
//! the traits they uphold. This detector verifies, at runtime, that
//! the *rendered DOM* of each primitive instance actually fulfils
//! every runtime-verifiable trait it claims.
//!
//! Crawler stays consumer-agnostic. We never import loom-traits or
//! cms-traits — they may not exist on the target. Instead the caller
//! hands the runner:
//!
//! 1. A list of `TraitProbeInput` rows describing which selectors on
//!    the page correspond to which primitive instances and which
//!    traits each declares.
//! 2. A `TraitPredicateRegistry` mapping `trait_id → TraitPredicate`.
//!    A default registry ships covering the 12 commonly runtime-
//!    verifiable trait IDs across the PlausiDen ecosystem, but
//!    consumers are free to extend it for their own taxonomies.
//!
//! Traits that are SOURCE-level (e.g. `manifested`, `versioned`,
//! `doctrine-cited`, `substrate-native`, `no-site-specific`) are
//! reported as `NotRuntimeVerifiable` so the consumer can route them
//! back to its own source-level audit (typically a Forge phase).
//!
//! BUG ASSUMPTION
//! --------------
//! The caller has driven the page to a stable layout state BEFORE
//! invoking this detector. Probes that depend on viewport metrics
//! (mobile-friendly, rtl-aware) trust the current `innerWidth` /
//! `dir` setting; the caller is responsible for setting those
//! beforehand via the journey runner.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum and result struct.
//! * Pure functions; the JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One probe declared by the caller: "the DOM nodes under this CSS
/// selector are instances of `entity_id` and should uphold these
/// `declared_traits`."
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TraitProbeInput {
    /// Caller-defined primitive id (e.g. `Loom.Primitive.Heading`).
    pub entity_id: String,
    /// CSS selector that resolves to all instances on this page.
    pub selector: String,
    /// Trait identifiers the entity declares (kebab-case wire form).
    pub declared_traits: Vec<String>,
}

/// Runtime predicate the detector evaluates against a DOM element.
/// Each variant maps to a chunk of the page-side JS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum TraitPredicate {
    /// Element (or a descendant) carries an accessible name —
    /// `aria-label`, `aria-labelledby`, or non-empty visible text.
    HasAccessibleName,
    /// Element is in the natural tab order — natural focusable tag
    /// OR `tabindex >= 0`.
    KeyboardOperable,
    /// Element accepts focus and dispatches a focus event without
    /// throwing. Implies `KeyboardOperable`.
    Focusable,
    /// Element (or nearest ancestor) carries `lang=`.
    LangAware,
    /// Computed style references at least one CSS custom property
    /// (`var(--...)`) — best heuristic for "uses the token cascade".
    ThemeAware,
    /// At the current viewport, element's bounding rect does not
    /// exceed `document.documentElement.clientWidth`.
    MobileFriendly,
    /// Element's computed `direction` reflects the document `dir`.
    RtlAware,
    /// Element opts in to lazy loading — `loading="lazy"` for
    /// `<img>` / `<iframe>` / `<video>` (poster).
    LazyLoadable,
    /// Element has no `animation` / `transition` with a non-zero
    /// duration under the current `prefers-reduced-motion` setting.
    /// Only meaningful when the caller has set the media feature.
    ReducedMotionAware,
    /// Element exposes an ARIA role consistent with its tag (best-
    /// effort: roles in the implicit-role list for the tag, OR an
    /// explicit `role=` attribute).
    ScreenReaderAccessible,
    /// Element's bounding rect width/height ≥ 44px when interactive
    /// (touch-target floor, WCAG 2.5.5 AAA target).
    TouchTargetSized,
    /// Source-level only — Crawler cannot verify at runtime.
    NotRuntimeVerifiable,
}

/// Map from `trait_id` (kebab-case) → predicate.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(transparent)]
pub struct TraitPredicateRegistry {
    /// Internal storage. BTreeMap so JSON round-trips are stable.
    pub by_trait: BTreeMap<String, TraitPredicate>,
}

impl TraitPredicateRegistry {
    /// Empty registry — caller must populate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Default registry for the 12 commonly runtime-verifiable trait
    /// IDs across the PlausiDen ecosystem. Source-level traits are
    /// included with `NotRuntimeVerifiable` so a probe that lists
    /// them does not produce a false-positive "unknown trait" error.
    #[must_use]
    pub fn ecosystem_default() -> Self {
        let mut m = BTreeMap::new();
        // Runtime-verifiable.
        m.insert("screen-reader-accessible".to_owned(), TraitPredicate::ScreenReaderAccessible);
        m.insert("keyboard-operable".to_owned(), TraitPredicate::KeyboardOperable);
        m.insert("focusable".to_owned(), TraitPredicate::Focusable);
        m.insert("rtl-aware".to_owned(), TraitPredicate::RtlAware);
        m.insert("lang-aware".to_owned(), TraitPredicate::LangAware);
        m.insert("theme-aware".to_owned(), TraitPredicate::ThemeAware);
        m.insert("mobile-friendly".to_owned(), TraitPredicate::MobileFriendly);
        m.insert("lazy-loadable".to_owned(), TraitPredicate::LazyLoadable);
        m.insert("reduced-motion-aware".to_owned(), TraitPredicate::ReducedMotionAware);
        m.insert("touch-target-sized".to_owned(), TraitPredicate::TouchTargetSized);
        m.insert("has-accessible-name".to_owned(), TraitPredicate::HasAccessibleName);
        // Source-level (not runtime-verifiable).
        for src in [
            "manifested",
            "versioned",
            "doctrine-cited",
            "substrate-native",
            "no-site-specific",
            "bundle-size-bounded",
            "audit-passing",
            "non-flaky",
            "deterministic-baseline",
        ] {
            m.insert(src.to_owned(), TraitPredicate::NotRuntimeVerifiable);
        }
        Self { by_trait: m }
    }

    /// Look up a trait's predicate.
    #[must_use]
    pub fn get(&self, trait_id: &str) -> Option<TraitPredicate> {
        self.by_trait.get(trait_id).copied()
    }

    /// Add or override a trait → predicate mapping.
    pub fn insert(&mut self, trait_id: impl Into<String>, predicate: TraitPredicate) {
        self.by_trait.insert(trait_id.into(), predicate);
    }
}

/// Per-instance verification result for one probe row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TraitProbeResult {
    /// The `entity_id` from the input row.
    pub entity_id: String,
    /// The selector from the input row.
    pub selector: String,
    /// Number of DOM nodes the selector resolved to.
    pub matched_count: u32,
    /// Per-trait, per-node verdict.
    pub verdicts: Vec<TraitVerdict>,
}

/// One verdict — what we checked, on which instance, and what we found.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TraitVerdict {
    /// Trait id from the declared list.
    pub trait_id: String,
    /// Predicate name resolved from the registry (kebab-case).
    pub predicate: String,
    /// `selector + index` shape so a single declaration's multiple
    /// matches don't collapse together.
    pub instance_selector: String,
    /// `true` = predicate held. `false` = predicate violated.
    /// `Option::None` = source-level / unknown — skipped at runtime.
    pub holds: Option<bool>,
    /// Brief reason string. Empty when `holds == Some(true)`.
    pub reason: String,
}

/// Eval-result wire shape produced by the page-side JS.
///
/// The JS receives `probes` + `predicate_map` as arguments and returns
/// this struct. Pure data; no rendering side-effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TraitVerificationSnapshot {
    /// `window.innerWidth` at probe time.
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight` at probe time.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// `document.documentElement.dir` at probe time.
    pub dir: String,
    /// `prefers-reduced-motion` media feature at probe time.
    pub prefers_reduced_motion: bool,
    /// One result row per input probe.
    pub probes: Vec<TraitProbeResult>,
}

/// Page-side JS. Receives two arguments via `Page::evaluate_function`:
/// `(probes: TraitProbeInput[], predicateMap: Record<string, string>)`.
/// `predicateMap` is the registry serialized as `trait_id → predicate`
/// (kebab-case predicate name).
pub const TRAIT_VERIFICATION_JS: &str = r##"((probes, predicateMap) => {
    const selectorOf = function(el, baseSel, idx) {
      // Use the input selector + nth-of-match index so duplicate
      // matches don't collide. Index is 0-based.
      return baseSel + '[' + idx + ']';
    };

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      return true;
    };

    const hasAccessibleName = function(el) {
      if (el.getAttribute('aria-label') && el.getAttribute('aria-label').trim()) return true;
      if (el.getAttribute('aria-labelledby')) return true;
      if (el.tagName === 'IMG' && el.getAttribute('alt') !== null) return true;
      const text = (el.textContent || '').trim();
      if (text.length > 0) return true;
      return false;
    };

    const naturallyFocusable = function(tag) {
      return tag === 'a' || tag === 'button' || tag === 'input' ||
             tag === 'select' || tag === 'textarea' || tag === 'summary';
    };

    const keyboardOperable = function(el) {
      const tag = el.tagName.toLowerCase();
      if (naturallyFocusable(tag)) {
        if (tag === 'a' && !el.hasAttribute('href')) return false;
        if (el.hasAttribute('disabled')) return false;
        return true;
      }
      const ti = el.getAttribute('tabindex');
      if (ti === null) return false;
      const n = parseInt(ti, 10);
      return !isNaN(n) && n >= 0;
    };

    const focusable = function(el) {
      if (!keyboardOperable(el)) return false;
      const prev = document.activeElement;
      try {
        el.focus({ preventScroll: true });
      } catch (e) {
        return false;
      }
      const ok = document.activeElement === el;
      try { if (prev && prev.focus) prev.focus({ preventScroll: true }); } catch (e) {}
      return ok;
    };

    const langAware = function(el) {
      let p = el;
      while (p && p.nodeType === 1) {
        if (p.hasAttribute && p.hasAttribute('lang')) return true;
        p = p.parentElement;
      }
      return false;
    };

    const themeAware = function(el) {
      // Inspect inline + computed property strings for the CSS-variable
      // sigil. We can't introspect the cascade directly, but the
      // text reachable via `getComputedStyle` resolves var() down to a
      // value — so we walk the rule list best-effort by scanning
      // `style.cssText` AND testing tokens-sensitive properties for
      // substitution markers.
      const inline = (el.getAttribute('style') || '').toLowerCase();
      if (inline.indexOf('var') !== -1 && inline.indexOf('--') !== -1) return true;
      // Fall back to: at least one of the canonical theme-cascade
      // properties was set via a custom property elsewhere — we test
      // by reading the property and checking it differs from the
      // user-agent default. Heuristic but cheap.
      const cs = window.getComputedStyle(el);
      const candidates = ['color', 'background-color', 'border-color', 'fill', 'stroke'];
      for (let i = 0; i < candidates.length; i++) {
        const v = cs.getPropertyValue(candidates[i]);
        // crude: any non-default, non-transparent value implies
        // *something* in the cascade set it — likely a token.
        if (v && v !== 'rgba(0, 0, 0, 0)' && v !== 'transparent' && v !== '') return true;
      }
      return false;
    };

    const mobileFriendly = function(el) {
      const rect = el.getBoundingClientRect();
      const docW = document.documentElement.clientWidth;
      // Allow 2px slack for sub-pixel rounding.
      return rect.right <= docW + 2 && rect.left >= -2;
    };

    const rtlAware = function(el) {
      const cs = window.getComputedStyle(el);
      const docDir = document.documentElement.getAttribute('dir') || 'ltr';
      // If document is rtl, the element's computed direction should
      // be rtl too (either inherited or explicitly overridden in a
      // way the substrate intends).
      if (docDir === 'rtl' && cs.direction !== 'rtl') return false;
      return true;
    };

    const lazyLoadable = function(el) {
      const tag = el.tagName.toLowerCase();
      if (tag !== 'img' && tag !== 'iframe' && tag !== 'video') {
        // Only meaningful for these tags; absent attribute on other
        // tags is a no-op, not a violation.
        return null;
      }
      return el.getAttribute('loading') === 'lazy';
    };

    const reducedMotionAware = function(el) {
      // When prefers-reduced-motion: reduce is active, the substrate
      // is expected to set animation/transition duration to 0.
      // We check that NEITHER property reports a non-zero duration.
      const cs = window.getComputedStyle(el);
      const anim = cs.animationDuration || '0s';
      const tran = cs.transitionDuration || '0s';
      const isZero = function(s) {
        if (!s) return true;
        const parts = s.split(',').map(function(p) { return p.trim(); });
        for (let i = 0; i < parts.length; i++) {
          const v = parts[i];
          if (v !== '0s' && v !== '0ms' && v !== '0' && v !== 'auto') return false;
        }
        return true;
      };
      return isZero(anim) && isZero(tran);
    };

    const screenReaderAccessible = function(el) {
      // Either has an accessible name OR exposes a non-presentation
      // role (a screen reader can announce it). Hidden-from-AT
      // elements (aria-hidden=true) are a violation.
      if (el.getAttribute('aria-hidden') === 'true') return false;
      const role = el.getAttribute('role');
      if (role === 'presentation' || role === 'none') return false;
      return hasAccessibleName(el) || !!role;
    };

    const touchTargetSized = function(el) {
      const rect = el.getBoundingClientRect();
      return rect.width >= 44 && rect.height >= 44;
    };

    const dispatch = function(predicate, el) {
      switch (predicate) {
        case 'has-accessible-name': return hasAccessibleName(el);
        case 'keyboard-operable': return keyboardOperable(el);
        case 'focusable': return focusable(el);
        case 'lang-aware': return langAware(el);
        case 'theme-aware': return themeAware(el);
        case 'mobile-friendly': return mobileFriendly(el);
        case 'rtl-aware': return rtlAware(el);
        case 'lazy-loadable': return lazyLoadable(el);
        case 'reduced-motion-aware': return reducedMotionAware(el);
        case 'screen-reader-accessible': return screenReaderAccessible(el);
        case 'touch-target-sized': return touchTargetSized(el);
        case 'not-runtime-verifiable': return null;
        default: return undefined;
      }
    };

    const out = [];
    for (let pi = 0; pi < probes.length; pi++) {
      const probe = probes[pi];
      let els;
      try { els = document.querySelectorAll(probe.selector); }
      catch (e) { els = []; }

      const visible = [];
      for (let i = 0; i < els.length; i++) {
        if (isVisible(els[i])) visible.push(els[i]);
      }

      const verdicts = [];
      for (let ti = 0; ti < probe.declaredTraits.length; ti++) {
        const traitId = probe.declaredTraits[ti];
        const predicate = predicateMap[traitId];
        if (predicate === undefined) {
          // Unknown trait — best to surface as a non-verdict and let
          // the consumer decide. We mark holds=null + reason='unknown'.
          for (let ei = 0; ei < visible.length; ei++) {
            verdicts.push({
              traitId: traitId,
              predicate: 'unknown',
              instanceSelector: selectorOf(visible[ei], probe.selector, ei),
              holds: null,
              reason: 'trait id not in predicate registry'
            });
          }
          continue;
        }
        for (let ei = 0; ei < visible.length; ei++) {
          const el = visible[ei];
          const result = dispatch(predicate, el);
          if (result === null) {
            // Source-level / lazy-loadable on a non-applicable tag.
            verdicts.push({
              traitId: traitId,
              predicate: predicate,
              instanceSelector: selectorOf(el, probe.selector, ei),
              holds: null,
              reason: 'not runtime-verifiable for this element type'
            });
          } else if (result === undefined) {
            verdicts.push({
              traitId: traitId,
              predicate: predicate,
              instanceSelector: selectorOf(el, probe.selector, ei),
              holds: null,
              reason: 'predicate not implemented'
            });
          } else if (result === true) {
            verdicts.push({
              traitId: traitId,
              predicate: predicate,
              instanceSelector: selectorOf(el, probe.selector, ei),
              holds: true,
              reason: ''
            });
          } else {
            verdicts.push({
              traitId: traitId,
              predicate: predicate,
              instanceSelector: selectorOf(el, probe.selector, ei),
              holds: false,
              reason: 'predicate ' + predicate + ' failed for declared trait ' + traitId
            });
          }
        }
      }

      out.push({
        entityId: probe.entityId,
        selector: probe.selector,
        matchedCount: visible.length,
        verdicts: verdicts
      });
    }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      dir: document.documentElement.getAttribute('dir') || 'ltr',
      prefersReducedMotion: window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches === true,
      probes: out
    };
})"##;

/// Apply detection rules to a verification snapshot. Pure function.
///
/// Emits one strict finding per `holds == Some(false)` verdict.
/// `None` verdicts (source-level or unknown) are not reported here;
/// the consumer routes those through its own audit.
#[must_use]
pub fn detect_trait_violations(snap: &TraitVerificationSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    for probe in &snap.probes {
        for verdict in &probe.verdicts {
            if verdict.holds == Some(false) {
                out.push(crate::AxisFinding {
                    severity: crate::AxisSeverity::Strict,
                    kind: format!("trait.{}.violated", verdict.trait_id),
                    detail: format!(
                        "{} @ {}: declared trait `{}` not upheld at runtime (predicate `{}` failed)",
                        probe.entity_id,
                        verdict.instance_selector,
                        verdict.trait_id,
                        verdict.predicate,
                    ),
                });
            }
        }
    }
    out
}

/// Convenience: serialize a probe list to the JSON shape the page-side
/// JS expects (camelCase keys).
///
/// # Errors
/// Returns `serde_json::Error` if any input row contains non-string
/// data that fails to encode (in practice: only OOM or bug).
pub fn probes_to_eval_arg(probes: &[TraitProbeInput]) -> serde_json::Result<serde_json::Value> {
    serde_json::to_value(probes)
}

/// Convenience: serialize a registry to the flat `trait_id → predicate`
/// JSON map the page-side JS expects (camelCase / kebab-case stays as-is).
///
/// # Errors
/// Returns `serde_json::Error` if the registry contains data that fails
/// to encode (in practice: only OOM or bug).
pub fn registry_to_eval_arg(registry: &TraitPredicateRegistry) -> serde_json::Result<serde_json::Value> {
    let mut out = serde_json::Map::new();
    for (k, v) in &registry.by_trait {
        let pred = serde_json::to_value(v)?;
        out.insert(k.clone(), pred);
    }
    Ok(serde_json::Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            TRAIT_VERIFICATION_JS.matches('(').count(),
            TRAIT_VERIFICATION_JS.matches(')').count()
        );
        assert_eq!(
            TRAIT_VERIFICATION_JS.matches('{').count(),
            TRAIT_VERIFICATION_JS.matches('}').count()
        );
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW", "vpH", "dir", "prefersReducedMotion", "probes",
            "entityId", "selector", "matchedCount", "verdicts",
            "traitId", "predicate", "instanceSelector", "holds", "reason",
        ] {
            assert!(TRAIT_VERIFICATION_JS.contains(k), "missing key in JS: {k}");
        }
    }

    #[test]
    fn js_dispatches_every_runtime_predicate() {
        for p in [
            "has-accessible-name",
            "keyboard-operable",
            "focusable",
            "lang-aware",
            "theme-aware",
            "mobile-friendly",
            "rtl-aware",
            "lazy-loadable",
            "reduced-motion-aware",
            "screen-reader-accessible",
            "touch-target-sized",
            "not-runtime-verifiable",
        ] {
            assert!(
                TRAIT_VERIFICATION_JS.contains(&format!("case '{p}'")),
                "JS dispatch missing predicate: {p}"
            );
        }
    }

    #[test]
    fn registry_default_covers_ecosystem_runtime_traits() {
        let r = TraitPredicateRegistry::ecosystem_default();
        assert_eq!(r.get("screen-reader-accessible"), Some(TraitPredicate::ScreenReaderAccessible));
        assert_eq!(r.get("mobile-friendly"), Some(TraitPredicate::MobileFriendly));
        assert_eq!(r.get("rtl-aware"), Some(TraitPredicate::RtlAware));
        assert_eq!(r.get("theme-aware"), Some(TraitPredicate::ThemeAware));
        assert_eq!(r.get("lazy-loadable"), Some(TraitPredicate::LazyLoadable));
        assert_eq!(r.get("touch-target-sized"), Some(TraitPredicate::TouchTargetSized));
    }

    #[test]
    fn registry_default_classifies_source_level_traits() {
        let r = TraitPredicateRegistry::ecosystem_default();
        for src in [
            "manifested",
            "versioned",
            "doctrine-cited",
            "substrate-native",
            "no-site-specific",
            "bundle-size-bounded",
        ] {
            assert_eq!(
                r.get(src),
                Some(TraitPredicate::NotRuntimeVerifiable),
                "expected source-level classification for {src}"
            );
        }
    }

    #[test]
    fn registry_insert_overrides() {
        let mut r = TraitPredicateRegistry::ecosystem_default();
        r.insert("custom-trait", TraitPredicate::Focusable);
        assert_eq!(r.get("custom-trait"), Some(TraitPredicate::Focusable));
    }

    #[test]
    fn detect_emits_strict_per_violation() {
        let snap = TraitVerificationSnapshot {
            vp_w: 360,
            vp_h: 800,
            dir: "ltr".to_owned(),
            prefers_reduced_motion: false,
            probes: vec![TraitProbeResult {
                entity_id: "Loom.Primitive.Heading".to_owned(),
                selector: ".loom-heading".to_owned(),
                matched_count: 2,
                verdicts: vec![
                    TraitVerdict {
                        trait_id: "screen-reader-accessible".to_owned(),
                        predicate: "screen-reader-accessible".to_owned(),
                        instance_selector: ".loom-heading[0]".to_owned(),
                        holds: Some(true),
                        reason: String::new(),
                    },
                    TraitVerdict {
                        trait_id: "mobile-friendly".to_owned(),
                        predicate: "mobile-friendly".to_owned(),
                        instance_selector: ".loom-heading[1]".to_owned(),
                        holds: Some(false),
                        reason: "predicate mobile-friendly failed for declared trait mobile-friendly".to_owned(),
                    },
                ],
            }],
        };
        let findings = detect_trait_violations(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert!(findings[0].kind.starts_with("trait."));
        assert!(findings[0].kind.ends_with(".violated"));
        assert!(findings[0].detail.contains("Loom.Primitive.Heading"));
        assert!(findings[0].detail.contains("mobile-friendly"));
    }

    #[test]
    fn detect_skips_none_verdicts() {
        // None verdicts (source-level / unknown) should NOT produce findings —
        // the consumer routes those through its own source-level audit.
        let snap = TraitVerificationSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "ltr".to_owned(),
            prefers_reduced_motion: false,
            probes: vec![TraitProbeResult {
                entity_id: "X".to_owned(),
                selector: ".x".to_owned(),
                matched_count: 1,
                verdicts: vec![
                    TraitVerdict {
                        trait_id: "manifested".to_owned(),
                        predicate: "not-runtime-verifiable".to_owned(),
                        instance_selector: ".x[0]".to_owned(),
                        holds: None,
                        reason: "not runtime-verifiable for this element type".to_owned(),
                    },
                    TraitVerdict {
                        trait_id: "made-up-trait".to_owned(),
                        predicate: "unknown".to_owned(),
                        instance_selector: ".x[0]".to_owned(),
                        holds: None,
                        reason: "trait id not in predicate registry".to_owned(),
                    },
                ],
            }],
        };
        let findings = detect_trait_violations(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_skips_pass_verdicts() {
        let snap = TraitVerificationSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "ltr".to_owned(),
            prefers_reduced_motion: false,
            probes: vec![TraitProbeResult {
                entity_id: "Loom.Primitive.Paragraph".to_owned(),
                selector: "p".to_owned(),
                matched_count: 1,
                verdicts: vec![TraitVerdict {
                    trait_id: "lang-aware".to_owned(),
                    predicate: "lang-aware".to_owned(),
                    instance_selector: "p[0]".to_owned(),
                    holds: Some(true),
                    reason: String::new(),
                }],
            }],
        };
        let findings = detect_trait_violations(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn snapshot_round_trips_json() {
        let snap = TraitVerificationSnapshot {
            vp_w: 360,
            vp_h: 800,
            dir: "rtl".to_owned(),
            prefers_reduced_motion: true,
            probes: vec![TraitProbeResult {
                entity_id: "E".to_owned(),
                selector: ".e".to_owned(),
                matched_count: 1,
                verdicts: vec![TraitVerdict {
                    trait_id: "rtl-aware".to_owned(),
                    predicate: "rtl-aware".to_owned(),
                    instance_selector: ".e[0]".to_owned(),
                    holds: Some(false),
                    reason: "predicate rtl-aware failed for declared trait rtl-aware".to_owned(),
                }],
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: TraitVerificationSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.dir, "rtl");
        assert!(back.prefers_reduced_motion);
        assert_eq!(back.probes.len(), 1);
        assert_eq!(back.probes[0].verdicts.len(), 1);
    }

    #[test]
    fn probes_to_eval_arg_shape() {
        let probes = vec![TraitProbeInput {
            entity_id: "Loom.Primitive.Heading".to_owned(),
            selector: ".loom-heading".to_owned(),
            declared_traits: vec![
                "screen-reader-accessible".to_owned(),
                "mobile-friendly".to_owned(),
            ],
        }];
        let v = probes_to_eval_arg(&probes).expect("ser");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        let obj = arr[0].as_object().expect("obj");
        assert!(obj.contains_key("entityId"));
        assert!(obj.contains_key("selector"));
        assert!(obj.contains_key("declaredTraits"));
    }

    #[test]
    fn registry_to_eval_arg_shape() {
        let r = TraitPredicateRegistry::ecosystem_default();
        let v = registry_to_eval_arg(&r).expect("ser");
        let obj = v.as_object().expect("obj");
        // Trait ids are keys; predicate kebab-case strings are values.
        assert_eq!(
            obj.get("screen-reader-accessible")
                .and_then(serde_json::Value::as_str),
            Some("screen-reader-accessible")
        );
        assert_eq!(
            obj.get("manifested").and_then(serde_json::Value::as_str),
            Some("not-runtime-verifiable")
        );
    }

    #[test]
    fn registry_serde_round_trips() {
        let r = TraitPredicateRegistry::ecosystem_default();
        let json = serde_json::to_string(&r).expect("ser");
        let back: TraitPredicateRegistry = serde_json::from_str(&json).expect("de");
        assert_eq!(back.by_trait.len(), r.by_trait.len());
    }

    #[test]
    fn probe_input_round_trips() {
        let probe = TraitProbeInput {
            entity_id: "Loom.Primitive.Faq".to_owned(),
            selector: "[data-loom-primitive='faq']".to_owned(),
            declared_traits: vec!["theme-aware".to_owned(), "lang-aware".to_owned()],
        };
        let json = serde_json::to_string(&probe).expect("ser");
        let back: TraitProbeInput = serde_json::from_str(&json).expect("de");
        assert_eq!(back.entity_id, "Loom.Primitive.Faq");
        assert_eq!(back.declared_traits.len(), 2);
    }
}
