// Small fuzzy matcher for the command palette and sidebar search.
// Subsequence match with a score that favors word starts and early hits.

export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 0;
  if (q.length > t.length) return null;

  let score = 0;
  let ti = 0;
  let streak = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi];
    const found = t.indexOf(ch, ti);
    if (found === -1) return null;
    // Word-start matches matter most (initials like "pvl" -> "prod vps ldn").
    if (found === 0 || /[\s._@:\/-]/.test(t[found - 1] ?? "")) score += 8;
    // Consecutive matches mean the user is typing a real prefix.
    if (found === ti && qi > 0) {
      streak += 1;
      score += 4 + streak;
    } else {
      streak = 0;
    }
    // Earlier matches rank higher.
    score += Math.max(0, 6 - found / 4);
    ti = found + 1;
  }
  // Shorter haystacks are more specific.
  score += Math.max(0, 10 - t.length / 6);
  return score;
}

export function fuzzyFilter<T>(items: T[], getText: (item: T) => string, query: string): T[] {
  const scored: { item: T; score: number }[] = [];
  for (const item of items) {
    const score = fuzzyScore(query, getText(item));
    if (score !== null) scored.push({ item, score });
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.item);
}
