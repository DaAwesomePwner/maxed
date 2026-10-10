/// RealmShark Projectile.damageWithDefense, with wide intermediates to avoid
/// overflow on hostile inputs. No names are inferred for unverified masks.
pub fn damage_with_defense(damage: i32, piercing: bool, defense: i32, conditions: [i32; 2]) -> i32 {
    if damage <= 0 {
        return 0;
    }
    let mut defense = i64::from(defense);
    if piercing || conditions[0] & 0x4000000 != 0 {
        defense = 0;
    } else if conditions[0] & 0x2000000 != 0 {
        defense = (defense as f64 * 1.5) as i64;
    }
    if conditions[1] & 0x20000 != 0 {
        defense -= 20;
    }
    let mut n = (i64::from(damage) * 2 / 20).max(i64::from(damage) - defense);
    if conditions[0] & 0x1000000 != 0 {
        n = 0;
    }
    if conditions[1] & 0x8 != 0 {
        n = (n as f64 * 0.9) as i64;
    }
    if conditions[1] & 0x40 != 0 {
        n = (n as f64 * 1.25) as i64;
    }
    n.clamp(0, i64::from(i32::MAX)) as i32
}

pub(crate) struct Rng(pub i64);
impl Rng {
    // RealmShark util.RNG.next; signed input seed, unsigned final subtraction.
    pub fn next(&mut self) -> i64 {
        let right = (self.0 >> 16) * 0x41a7;
        let sum = (self.0 & 0xffff) * 0x41a7 + ((right & 0x7fff) << 16) + (right >> 15);
        self.0 = if sum <= 0x7fff_ffff {
            sum
        } else {
            i64::from((sum as i32).wrapping_sub(0x7fff_ffff) as u32)
        };
        self.0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reference_seed_sequence() {
        let mut rng = super::Rng(1);
        for n in [16807, 282475249, 1622650073, 984943658, 1144108930] {
            assert_eq!(rng.next(), n);
        }
    }
}
