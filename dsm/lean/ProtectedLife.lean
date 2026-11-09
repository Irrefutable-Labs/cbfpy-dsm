/-
  Protected life: the rules the enforcer checks, on natural numbers.

  This proves the mathematics of the rules as `ethical_world::check` writes them. It is a model:
  nothing here is machine-linked to the Rust, whose correspondence rests on its tests. Run with
  `lean ProtectedLife.lean`.

  `unknown_counts_as_present`: a cell never looked into is never clear, whatever is or is not
  there. `clear_means_seen`: a clear cell has had enough recent looks, each fine enough to see the
  smallest protected being, and no protected being was seen there. `reach_all_seen`: an allowed
  act reaches only such cells. `stops_within_seen`: a van admitted at a speed can stop within road
  it has seen clear. `ratchet`: a chain of allowed steps never leaves anyone below where they were,
  so an improvement becomes the floor.
-/

namespace ProtectedLife

/-- One look into a cell: the step it was made at, and the finest detail it resolved, in mm. -/
structure Look where
  step : Nat
  res : Nat

/-- A look that still counts at step `now`, fine enough to see something `smallest` mm long. -/
def good (now freshFor smallest : Nat) (l : Look) : Bool :=
  decide (now - l.step ≤ freshFor) && decide (l.res ≤ smallest)

def clearLooks (now freshFor smallest : Nat) (looks : List Look) : Nat :=
  (looks.filter (good now freshFor smallest)).length

/-- A cell is clear for an act needing `need` looks if no protected being was seen there and it
has at least that many good looks. -/
def cellClear (now freshFor smallest need : Nat) (occupied : Bool) (looks : List Look) : Bool :=
  !occupied && decide (need ≤ clearLooks now freshFor smallest looks)

theorem unknown_counts_as_present (now freshFor smallest need : Nat) (occupied : Bool)
    (hneed : 1 ≤ need) :
    cellClear now freshFor smallest need occupied [] = false := by
  simp [cellClear, clearLooks]
  omega

theorem a_seen_being_is_never_clear (now freshFor smallest need : Nat) (looks : List Look) :
    cellClear now freshFor smallest need true looks = false := by
  simp [cellClear]

theorem clear_means_seen (now freshFor smallest need : Nat) (occupied : Bool) (looks : List Look)
    (hneed : 1 ≤ need) (h : cellClear now freshFor smallest need occupied looks = true) :
    occupied = false ∧ ∃ l ∈ looks, now - l.step ≤ freshFor ∧ l.res ≤ smallest := by
  simp [cellClear] at h
  obtain ⟨hocc, hcount⟩ := h
  refine ⟨hocc, ?_⟩
  have hpos : 0 < (looks.filter (good now freshFor smallest)).length := by
    simp [clearLooks] at hcount
    omega
  obtain ⟨l, hl⟩ := List.exists_mem_of_length_pos hpos
  rw [List.mem_filter] at hl
  obtain ⟨hmem, hgood⟩ := hl
  simp only [good, Bool.and_eq_true, decide_eq_true_eq] at hgood
  exact ⟨l, hmem, hgood.1, hgood.2⟩

/-- An act's reach, as each cell's occupancy and looks: allowed only if every cell is clear. -/
def reachClear (now freshFor smallest need : Nat) (reach : List (Bool × List Look)) : Bool :=
  reach.all (fun c => cellClear now freshFor smallest need c.1 c.2)

theorem reach_all_seen (now freshFor smallest need : Nat) (reach : List (Bool × List Look))
    (hneed : 1 ≤ need) (h : reachClear now freshFor smallest need reach = true) :
    ∀ c ∈ reach, c.1 = false ∧ ∃ l ∈ c.2, now - l.step ≤ freshFor ∧ l.res ≤ smallest := by
  intro c hc
  have hall := List.all_eq_true.mp h c hc
  exact clear_means_seen now freshFor smallest need c.1 c.2 hneed hall

/-- Integer division rounded up, as the engine computes it. -/
def ceilDiv (x k : Nat) : Nat := (x + k - 1) / k

theorem le_mul_ceilDiv (x k : Nat) (hk : 0 < k) : x ≤ k * ceilDiv x k := by
  unfold ceilDiv
  have hmod := Nat.mod_lt (x + k - 1) hk
  have hdiv := Nat.div_add_mod (x + k - 1) k
  generalize (x + k - 1) / k = q at *
  generalize (x + k - 1) % k = r at *
  generalize k * q = m at *
  omega

/-- The engine's stopping distance in cm, `v² / 2a` rounded up, and in whole cells of 100 cm. -/
def stopCm (v a : Nat) : Nat := ceilDiv (v * v) (2 * a)
def stopCells (v a : Nat) : Nat := ceilDiv (stopCm v a) 100

/-- A speed the engine admits, with `d` cells of road seen clear ahead, lets the van stop within
them: `v² ≤ 2a · 100d`, the braking barrier `100d - v²/2a ≥ 0`. -/
theorem stops_within_seen (v a d : Nat) (ha : 0 < a) (h : stopCells v a ≤ d) :
    v * v ≤ 2 * a * (100 * d) := by
  have h1 : v * v ≤ 2 * a * stopCm v a := le_mul_ceilDiv (v * v) (2 * a) (by omega)
  have h2 : stopCm v a ≤ 100 * stopCells v a := le_mul_ceilDiv (stopCm v a) 100 (by omega)
  have h3 : 100 * stopCells v a ≤ 100 * d := Nat.mul_le_mul_left 100 h
  calc v * v ≤ 2 * a * stopCm v a := h1
    _ ≤ 2 * a * (100 * stopCells v a) := Nat.mul_le_mul_left _ h2
    _ ≤ 2 * a * (100 * d) := Nat.mul_le_mul_left _ h3

/-- Each allowed step leaves a level where it was or higher. -/
def neverLowers : List Nat → Prop
  | [] => True
  | [_] => True
  | a :: b :: rest => a ≤ b ∧ neverLowers (b :: rest)

/-- Over any chain of allowed steps, every later level is at least every earlier one: an
improvement is the floor from then on, and going back is refused. -/
theorem ratchet : ∀ (levels : List Nat) (i j : Nat) (hi : i < levels.length)
    (hj : j < levels.length), neverLowers levels → i ≤ j →
    levels.get ⟨i, hi⟩ ≤ levels.get ⟨j, hj⟩
  | [], _, _, hi, _, _, _ => absurd hi (Nat.not_lt_zero _)
  | [_], i, j, hi, hj, _, _ => by
    have : i = 0 := by simp at hi; omega
    have : j = 0 := by simp at hj; omega
    subst_vars; exact Nat.le_refl _
  | a :: b :: rest, i, j, hi, hj, h, hij => by
    obtain ⟨hab, hrest⟩ := h
    cases i with
    | zero =>
      cases j with
      | zero => exact Nat.le_refl _
      | succ j =>
        have hj' : j < (b :: rest).length := by simp at hj ⊢; omega
        have := ratchet (b :: rest) 0 j (by simp) hj' hrest (Nat.zero_le _)
        simp at this ⊢
        exact Nat.le_trans hab this
    | succ i =>
      cases j with
      | zero => omega
      | succ j =>
        have hi' : i < (b :: rest).length := by simp at hi ⊢; omega
        have hj' : j < (b :: rest).length := by simp at hj ⊢; omega
        have := ratchet (b :: rest) i j hi' hj' hrest (by omega)
        simpa using this

/-! The hypotheses are satisfiable, so none of the theorems above holds vacuously. -/

-- Two recent looks at 5 mm clear a cell for a lethal act; one does not; a coarse look does not.
example : cellClear 5 3 12 2 false [⟨4, 5⟩, ⟨5, 5⟩] = true := by decide
example : cellClear 5 3 12 2 false [⟨5, 5⟩] = false := by decide
example : cellClear 5 3 12 1 false [⟨5, 50⟩] = false := by decide
-- A look older than the window does not count.
example : cellClear 9 3 12 1 false [⟨5, 5⟩] = false := by decide
-- 10 m/s with 5 m/s² braking needs 10 m: ten cells, as the van scenario prints.
example : stopCells 1000 500 = 10 := by decide
example : stopCells 1400 500 = 20 := by decide
example : neverLowers [1, 2, 2, 3] := by simp [neverLowers]

end ProtectedLife
