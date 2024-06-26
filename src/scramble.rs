use std::{
    array,
    cmp::Ordering,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
};

/// Merges 2 arrays of same length into 1 using a given closure.
///
/// Basically `Iterator::zip(a, b).map(f)` if `a` and `b` were iterators.
fn zip_with<F, const N: usize>(a: &[u8; N], b: &[u8; N], f: F) -> [u8; N]
where
    F: Fn(u8, u8) -> u8,
{
    array::from_fn(|i| unsafe {
        let a = a.get_unchecked(i);
        let b = b.get_unchecked(i);
        f(*a, *b)
    })
}

/// Creates a bitmask with specified amount of leading zeros and the rest set to 1.
fn bitmask<const N: usize>(zero_bits: usize) -> [u8; N] {
    array::from_fn(|index| match index.cmp(&(zero_bits / 8)) {
        Ordering::Less => 0b00000000,
        Ordering::Equal => 0b11111111 >> (zero_bits % 8),
        Ordering::Greater => 0b11111111,
    })
}

/// Defines a common interface for encrypting a 128-bit data.
///
/// Although AES-128 is commonly used by CryptoPAn implementations, any 128-bit [block cipher]
/// can be used as a encryption backend. As a result, this library is designed so that [`Scrambler`]
/// is generic over [`Encrypter`].
///
/// It is the implementer's responsibility to ensure that the encrypter is:
///
/// - **Deterministic**: Always produces same output when given same input.
/// - **Secure**: Cryptographically secure, to prevent unauthorized decryption.
/// - **Efficient**: The encryption happens frequently during an anonymization\
///   (128 times per each IPv6 address) and needs to be reasonably fast.
///
/// [block cipher]: https://en.wikipedia.org/wiki/Block_cipher
pub trait Encrypter: Sized {
    /// Initializes an [`Encrypter`] from a 128-bit key.
    fn from_key(key: &[u8; 16]) -> Self;

    /// Encrypts a 128-bit block data.
    ///
    /// # Note
    ///
    /// Cipher implementations often require mutable access to its internal state during an
    /// encryption. In these cases, [interior mutability][std::cell] such as `UnsafeCell` will have
    /// to be used and the implementer must ensure that the output of this method is deterministic.
    fn encrypt(&self, input: &[u8; 16]) -> [u8; 16];
}

pub struct Scrambler<E: Encrypter> {
    encrypter: E,
    padding: [u8; 16],
}

impl<E: Encrypter> Scrambler<E> {
    pub fn new(key: &[u8; 32]) -> Self {
        let enc_key: &[u8; 16] = key[..16].try_into().unwrap();
        let pad_key: &[u8; 16] = key[16..].try_into().unwrap();
        let encrypter = Encrypter::from_key(enc_key);

        Self::with_encrypter(encrypter, pad_key)
    }

    pub fn with_encrypter(encrypter: E, padding: &[u8; 16]) -> Self {
        let padding = encrypter.encrypt(padding);
        Self { encrypter, padding }
    }

    pub fn scramble(&self, bits: &[u8; 16], rng: Range<usize>) -> [u8; 16] {
        if rng.end > 128 {
            panic!("`max_bits` should be less than 128");
        }

        let mut result: [u8; 16] = [0; 16];
        for i in rng {
            // first `i` bits from `bits`, the rest from `padding`
            // padded = (bits & !mask) | (self.padding & mask)
            let padded = {
                let mask = bitmask(i);
                let bits = zip_with(&mask, bits, |m, b| !m & b);
                let padding = zip_with(&mask, &self.padding, |m, p| m & p);
                zip_with(&bits, &padding, |b, p| b | p)
            };

            // put the first bit of the encrypted to the `i`th bit of the result
            let encrypted = self.encrypter.encrypt(&padded);
            result[i / 8] |= (encrypted[0] & 0b10000000) >> (i % 8);
        }

        zip_with(bits, &result, |b, r| b ^ r)
    }

    pub fn scramble_ip(&self, addr: IpAddr) -> IpAddr {
        // FEAT: option to preserve certain IP ranges
        match addr {
            IpAddr::V4(addr) => self.scramble_ipv4(addr).into(),
            IpAddr::V6(addr) => self.scramble_ipv6(addr).into(),
        }
    }

    pub fn scramble_ipv4(&self, addr: Ipv4Addr) -> Ipv4Addr {
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&addr.octets());

        let anonymized = self.scramble(&bytes, 0..32);
        let truncated: [u8; 4] = anonymized[..4].try_into().unwrap();

        truncated.into()
    }

    pub fn scramble_ipv6(&self, addr: Ipv6Addr) -> Ipv6Addr {
        let bytes = addr.octets();
        self.scramble(&bytes, 0..128).into()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_bitmask() {
        let mask: [u8; 2] = bitmask(9);
        assert_eq!(mask, [0b00000000, 0b01111111]);
    }
}
