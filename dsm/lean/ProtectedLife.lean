/-
  Protected life: the rules the enforcer checks, on natural numbers.

  This proves the mathematics of the rules as `ethical_world::check` writes them. It is a model:
  nothing here is machine-linked to the Rust, whose correspondence rests on its tests. Run with
  `lean ProtectedLife.lean`.

  A data source (a room, a resident's record, a garden zone) is **established** for an act when it
  has at least as many recent reads as the act's worst case needs. `unknown_counts_as_present`: a
  source never read is never established for any act that needs a read, whatever is or is not
  there. `established_means_read`: an established source has a read within the window.
  `reach_all_read`: everything an allowed act affects has. `allowed_setting_harms_no_one`: an
  allowed heating setting is inside the band of everyone it counts. `ratchet`: a chain of allowed
  steps never leaves anyone below where they were, so an improvement becomes the floor.
-/

namespace ProtectedLife

/-- The reads of a source that still count at step `now`: each is the step it was made at. -/
def freshReads (now freshFor : Nat) (reads : List Nat) : Nat :=
  (reads.filter (fun r => decide (now - r ≤ freshFor))).length

/-- A source is established for an act needing `need` reads if it has that many fresh ones. -/
def established (now freshFor need : Nat) (reads : List Nat) : Bool :=
  decide (need ≤ freshReads now freshFor reads)

theorem unknown_counts_as_present (now freshFor need : Nat) (hneed : 1 ≤ need) :
    established now freshFor need [] = false := by
  simp [established, freshReads]
  omega

theorem established_means_read (now freshFor need : Nat) (reads : List Nat)
    (hneed : 1 ≤ need) (h : established now freshFor need reads = true) :
    ∃ r ∈ reads, now - r ≤ freshFor := by
  have h' : need ≤ freshReads now freshFor reads := by simpa [established] using h
  have hpos : 0 < (reads.filter (fun r => decide (now - r ≤ freshFor))).length := by
    unfold freshReads at h'
    omega
  obtain ⟨r, hr⟩ := List.exists_mem_of_length_pos hpos
  rw [List.mem_filter] at hr
  obtain ⟨hmem, hfresh⟩ := hr
  exact ⟨r, hmem, by simpa using hfresh⟩

/-- An act's reach, as each source's reads: allowed only if every one is established. -/
def reachEstablished (now freshFor need : Nat) (reach : List (List Nat)) : Bool :=
  reach.all (fun reads => established now freshFor need reads)

theorem reach_all_read (now freshFor need : Nat) (reach : List (List Nat))
    (hneed : 1 ≤ need) (h : reachEstablished now freshFor need reach = true) :
    ∀ reads ∈ reach, ∃ r ∈ reads, now - r ≤ freshFor := by
  intro reads hreads
  exact established_means_read now freshFor need reads hneed (List.all_eq_true.mp h reads hreads)

/-- A setting a room may be given: within the band every protected species there can bear.
With `bands` the bands of those known to be in the room, or of every protected species when the
room has not been read, the setting is allowed only inside all of them. -/
def safeFor (bands : List (Int × Int)) (c : Int) : Bool :=
  bands.all (fun b => decide (b.1 ≤ c) && decide (c ≤ b.2))

/-- An allowed setting harms no one whose band was counted: unknown counts as present, so a room
no one has read is held to every protected species' band at once. -/
theorem allowed_setting_harms_no_one (bands : List (Int × Int)) (c : Int)
    (h : safeFor bands c = true) : ∀ b ∈ bands, b.1 ≤ c ∧ c ≤ b.2 := by
  intro b hb
  have := List.all_eq_true.mp h b hb
  simpa using this

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

-- Two recent reads establish a source for a lethal act; one does not.
example : established 5 3 2 [4, 5] = true := by decide
example : established 5 3 2 [5] = false := by decide
-- A read older than the window does not count.
example : established 9 3 1 [5] = false := by decide
-- 22 °C is safe for a person and a dog; 29 °C is not safe for a person.
example : safeFor [(18, 27), (10, 30)] 22 = true := by decide
example : safeFor [(18, 27), (10, 30)] 29 = false := by decide
example : neverLowers [1, 2, 2, 3] := by simp [neverLowers]

end ProtectedLife
