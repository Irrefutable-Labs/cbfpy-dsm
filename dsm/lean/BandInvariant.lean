/-
  The no-escape argument for one band, on natural-number magnitudes per side.

  This proves the mathematics of the rule. It is a model: nothing here is machine-linked to the
  Rust engine, whose correspondence rests on its tests. Run with `lean BandInvariant.lean`.

  `no_escape` and `no_escape_coupled` are one side with no crossing. `no_escape_two_sided` adds
  crossing the center, with a separate stop per side. `restoration` assumes a fixed threshold.
-/

namespace Band

/-- Both tension curves stay within the headroom. -/
theorem linear_le_headroom (h num den : Nat) (hn : num ≤ den) :
    h * num / den ≤ h := by
  apply Nat.div_le_of_le_mul
  calc h * num ≤ h * den := Nat.mul_le_mul_left _ hn
    _ = den * h := Nat.mul_comm _ _

theorem quadratic_le_headroom (h limit num den : Nat)
    (hn : num ≤ den) (hh : h ≤ limit) :
    h * h * num / (limit * den) ≤ h := by
  apply Nat.div_le_of_le_mul
  calc h * h * num = h * num * h := Nat.mul_right_comm h h num
    _ ≤ limit * den * h := Nat.mul_le_mul_right _ (Nat.mul_le_mul hh hn)

/-- A curve grows with headroom, so allowance never grows as deviation does. -/
theorem linear_monotone (a b num den : Nat) (hab : a ≤ b) :
    a * num / den ≤ b * num / den :=
  Nat.div_le_div_right (Nat.mul_le_mul_right _ hab)

theorem quadratic_monotone (a b limit num den : Nat) (hab : a ≤ b) :
    a * a * num / (limit * den) ≤ b * b * num / (limit * den) :=
  Nat.div_le_div_right (Nat.mul_le_mul_right _ (Nat.mul_le_mul hab hab))

/-- The engine's allowance: nothing at the stop, otherwise the smaller of the room to the stop
    and whatever the curve gave. -/
def allow (stop x curve : Nat) : Nat :=
  if stop ≤ x then 0 else min (stop - x) curve

theorem step_stays_below_stop (stop x next curve : Nat)
    (hx : x ≤ stop) (hs : next ≤ x + allow stop x curve) : next ≤ stop := by
  unfold allow at hs
  split at hs <;> omega

/-- No number of accepted steps passes the stop: this is cumulative drift, ruled out for any
    length of walk and any curve values, however small each step is. -/
theorem no_escape (stop : Nat) (xs curve : Nat → Nat)
    (h0 : xs 0 ≤ stop)
    (hs : ∀ k, xs (k + 1) ≤ xs k + allow stop (xs k) (curve k)) :
    ∀ k, xs k ≤ stop := by
  intro k
  induction k with
  | zero => exact h0
  | succ k ih => exact step_stays_below_stop stop (xs k) (xs (k + 1)) (curve k) ih (hs k)

/-- With coupling the stop changes from step to step. As long as every effective stop is inside
    the hard limit, which clamping guarantees, no walk passes the hard limit. -/
theorem no_escape_coupled (hard : Nat) (xs stop curve : Nat → Nat)
    (h0 : xs 0 ≤ hard)
    (hstop : ∀ k, stop k ≤ hard)
    (hs : ∀ k, xs (k + 1) ≤ xs k + allow (stop k) (xs k) (curve k)) :
    ∀ k, xs k ≤ hard := by
  intro k
  induction k with
  | zero => exact h0
  | succ k ih =>
    have hk := hs k
    have hsk := hstop k
    unfold allow at hk
    split at hk <;> omega

/-- Crossing the center: whatever lands on the far side counts as outward from the center, under
    that side's own stop. A walk that stays on a side or crosses it, in any order and for any
    length, stays inside the stop of whichever side it is on. -/
theorem no_escape_two_sided (stopOf : Bool → Nat) (side : Nat → Bool) (mag curve : Nat → Nat)
    (h0 : mag 0 ≤ stopOf (side 0))
    (hstay : ∀ k, side (k + 1) = side k →
      mag (k + 1) ≤ mag k + allow (stopOf (side k)) (mag k) (curve k))
    (hcross : ∀ k, side (k + 1) ≠ side k →
      mag (k + 1) ≤ allow (stopOf (side (k + 1))) 0 (curve k)) :
    ∀ k, mag k ≤ stopOf (side k) := by
  intro k
  induction k with
  | zero => exact h0
  | succ k ih =>
    by_cases hs : side (k + 1) = side k
    · rw [hs]
      have hk := hstay k hs
      exact step_stays_below_stop _ (mag k) (mag (k + 1)) (curve k) ih hk
    · have hk := hcross k hs
      unfold allow at hk
      split at hk <;> omega

/-- Past the restoring threshold, each accepted step gives up at least `p`, and a state at or
    under the threshold stays there. So a band returns within a bounded number of steps. -/
theorem restoration (r p : Nat) (xs : Nat → Nat)
    (hin : ∀ k, xs k ≤ r → xs (k + 1) ≤ r)
    (hout : ∀ k, r < xs k → xs (k + 1) + p ≤ xs k) :
    ∀ k, xs k ≤ r ∨ xs k + k * p ≤ xs 0 := by
  intro k
  induction k with
  | zero => right; omega
  | succ k ih =>
    rcases ih with h | h
    · left; exact hin k h
    · by_cases hr : xs k ≤ r
      · left; exact hin k hr
      · right
        have := hout k (by omega)
        rw [Nat.succ_mul]
        omega

end Band

#print axioms Band.no_escape
#print axioms Band.no_escape_two_sided
#print axioms Band.no_escape_coupled
#print axioms Band.restoration
