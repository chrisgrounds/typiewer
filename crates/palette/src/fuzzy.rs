const MATCH_BASE: i32 = 16;
const CONSECUTIVE_BONUS: i32 = 24;
const BOUNDARY_BONUS: i32 = 20;
const CAMEL_BONUS: i32 = 18;
const FIRST_CHAR_BONUS: i32 = 12;
const EXACT_CASE_BONUS: i32 = 4;
const GAP_START_PENALTY: i32 = -5;
const GAP_EXTEND_PENALTY: i32 = -1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuzzyMatch {
  pub score: i32,
  pub indices: Vec<usize>,
}

fn lower_char(ch: char) -> char {
  ch.to_lowercase().next().unwrap_or(ch)
}

fn is_separator(ch: char) -> bool {
  matches!(ch, '_' | '-' | '/' | '\\' | '.' | ':' | ' ')
}

fn is_subsequence(query_lower: &[char], candidate_lower: &[char]) -> bool {
  let mut qi = 0;
  for &cc in candidate_lower {
    if query_lower.get(qi) == Some(&cc) {
      qi += 1;
      if qi == query_lower.len() {
        return true;
      }
    }
  }
  qi == query_lower.len()
}

fn char_bonus(candidate: &[char], ci: usize, query_ch: char) -> i32 {
  let Some(&cand_ch) = candidate.get(ci) else {
    return 0;
  };

  let mut bonus = MATCH_BASE;

  if ci == 0 {
    bonus += FIRST_CHAR_BONUS + BOUNDARY_BONUS;
  } else if let Some(&prev) = candidate.get(ci - 1) {
    if is_separator(prev) {
      bonus += BOUNDARY_BONUS;
    } else if prev.is_lowercase() && cand_ch.is_uppercase() {
      bonus += CAMEL_BONUS;
    }
  }

  if query_ch == cand_ch && query_ch.is_uppercase() {
    bonus += EXACT_CASE_BONUS;
  }

  bonus
}

/// Scores a case-insensitive subsequence match of `query` against `candidate` using
/// dynamic programming so consecutive runs and word-boundary alignments win over greedy matches.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
  let query_chars: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
  if query_chars.is_empty() {
    return Some(FuzzyMatch {
      score: 0,
      indices: Vec::new(),
    });
  }

  let candidate_chars: Vec<char> = candidate.chars().collect();
  if query_chars.len() > candidate_chars.len() {
    return None;
  }

  let query_lower: Vec<char> = query_chars.iter().map(|&c| lower_char(c)).collect();
  let candidate_lower: Vec<char> = candidate_chars.iter().map(|&c| lower_char(c)).collect();

  if !is_subsequence(&query_lower, &candidate_lower) {
    return None;
  }

  let q_len = query_chars.len();
  let c_len = candidate_chars.len();

  // `match_scores[qi * c_len + ci]` = best score when `query[qi]` matches `candidate[ci]`.
  let mut match_scores = vec![i32::MIN / 4; q_len * c_len];
  // `prev_col[qi * c_len + ci]` = candidate index that matched `query[qi - 1]`.
  let mut prev_col = vec![usize::MAX; q_len * c_len];

  for ci in 0..c_len {
    if candidate_lower[ci] == query_lower[0] {
      let leading_gap = if ci == 0 {
        0
      } else {
        GAP_START_PENALTY + GAP_EXTEND_PENALTY * ((ci - 1) as i32)
      };
      match_scores[ci] = char_bonus(&candidate_chars, ci, query_chars[0]) + leading_gap;
    }
  }

  for qi in 1..q_len {
    let prev_row_offset = (qi - 1) * c_len;
    let row_offset = qi * c_len;

    // Track the best non-consecutive predecessor score adjusted for gap extension.
    let mut best_gap_score = i32::MIN / 4;
    let mut best_gap_col = usize::MAX;

    for ci in qi..c_len {
      // Advance existing best_gap_score by one gap step.
      if best_gap_col != usize::MAX {
        best_gap_score += GAP_EXTEND_PENALTY;
      }

      // Consider starting a new gap from `ci - 2` (since `ci - 1` is consecutive, not a gap).
      if ci >= 2 {
        let gap_src_col = ci - 2;
        let gap_src_score = match_scores[prev_row_offset + gap_src_col];
        if gap_src_score > i32::MIN / 8 {
          let candidate_gap = gap_src_score + GAP_START_PENALTY;
          if candidate_gap > best_gap_score {
            best_gap_score = candidate_gap;
            best_gap_col = gap_src_col;
          }
        }
      }

      if candidate_lower[ci] != query_lower[qi] {
        continue;
      }

      let bonus = char_bonus(&candidate_chars, ci, query_chars[qi]);

      let consec_score = match_scores[prev_row_offset + (ci - 1)];
      let from_consec = if consec_score > i32::MIN / 8 {
        consec_score + bonus + CONSECUTIVE_BONUS
      } else {
        i32::MIN / 4
      };

      let from_gap = if best_gap_col != usize::MAX {
        best_gap_score + bonus
      } else {
        i32::MIN / 4
      };

      if from_consec >= from_gap {
        match_scores[row_offset + ci] = from_consec;
        prev_col[row_offset + ci] = ci - 1;
      } else {
        match_scores[row_offset + ci] = from_gap;
        prev_col[row_offset + ci] = best_gap_col;
      }
    }
  }

  let last_row_offset = (q_len - 1) * c_len;
  let mut best_end_col = usize::MAX;
  let mut best_end_score = i32::MIN / 4;

  for ci in (q_len - 1)..c_len {
    let score = match_scores[last_row_offset + ci];
    if score > best_end_score {
      best_end_score = score;
      best_end_col = ci;
    }
  }

  if best_end_col == usize::MAX {
    return None;
  }

  let mut indices = vec![0usize; q_len];
  let mut col = best_end_col;
  for qi in (0..q_len).rev() {
    indices[qi] = col;
    if qi > 0 {
      col = prev_col[qi * c_len + col];
    }
  }

  // Slight tie-breaker favoring shorter candidates (e.g. `Graph` over `GraphEdge`).
  let final_score = best_end_score - (c_len as i32) / 2;

  Some(FuzzyMatch {
    score: final_score,
    indices,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn empty_query_matches_everything_with_no_highlights() {
    let m = fuzzy_match("", "TypeNode");
    assert_eq!(
      m,
      Some(FuzzyMatch {
        score: 0,
        indices: vec![],
      })
    );
  }

  #[test]
  fn rejects_non_subsequence() {
    assert!(fuzzy_match("xyz", "TypeNode").is_none());
    assert!(fuzzy_match("TypeNodes", "TypeNode").is_none());
  }

  #[test]
  fn camel_case_initials_match_and_highlight_word_starts() {
    let m = fuzzy_match("tn", "TypeNode");
    assert!(m.is_some());
    if let Some(m) = m {
      assert_eq!(m.indices, vec![0, 4]);
    }
  }

  #[test]
  fn prefers_contiguous_match_over_scattered_prefix() {
    let m = fuzzy_match("app", "a_p_app");
    assert!(m.is_some());
    if let Some(m) = m {
      assert_eq!(m.indices, vec![4, 5, 6]);
    }
  }

  #[test]
  fn prefers_exact_shorter_name_over_longer_prefix_match() {
    let short = fuzzy_match("graph", "Graph");
    let long = fuzzy_match("graph", "GraphResolution");
    assert!(short.is_some() && long.is_some());
    if let (Some(short), Some(long)) = (short, long) {
      assert!(short.score > long.score);
    }
  }

  #[test]
  fn prefers_boundary_match_over_mid_word_match() {
    let boundary = fuzzy_match("id", "TypeId");
    let mid = fuzzy_match("id", "Width");
    assert!(boundary.is_some() && mid.is_some());
    if let (Some(boundary), Some(mid)) = (boundary, mid) {
      assert!(boundary.score > mid.score);
    }
  }
}
