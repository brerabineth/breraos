//! Wallet flow integration: what the shell/UI will actually drive.
//! Mirrors the promise: seed in, signatures out, nothing else.

use zeroize::Zeroizing;

use brera_core::error::CoreError;
use brera_core::wallet::mnemonic::WordCount;
use brera_core::wallet::SignerHandle;

#[test]
fn cold_wallet_lifecycle_end_to_end() {
    let (signer, _join) = SignerHandle::spawn().expect("spawn");

    // 1. generate
    let receipt = signer.generate(WordCount::TwentyFour).expect("generate");
    assert_eq!(receipt.mnemonic.split_whitespace().count(), 24);
    assert_eq!(receipt.derivation, "m/44'/60'/0'/0/*");

    // 2. before unlock: nothing usable
    assert!(matches!(signer.address(0), Err(CoreError::VaultLocked)));
    assert!(matches!(
        signer.sign(b"tx".to_vec(), 0),
        Err(CoreError::VaultLocked)
    ));

    // 3. unlock with the phrase
    let status = signer
        .unlock(Zeroizing::new(receipt.mnemonic.to_string()))
        .expect("unlock");
    assert!(status.unlocked);
    assert_eq!(status.fingerprint, Some(receipt.fingerprint.clone()));

    // 4. deterministic addresses across indices
    let a0 = signer.address(0).expect("addr0");
    let a0b = signer.address(0).expect("addr0 again");
    let a5 = signer.address(5).expect("addr5");
    assert_eq!(a0, a0b);
    assert_ne!(a0, a5);

    // 5. sign and bind receipt to the wallet
    let payload = b"breraos://sign?kind=test&seq=1";
    let r1 = signer.sign(payload.to_vec(), 0).expect("sign");
    assert_eq!(r1.address, a0);
    assert_eq!(r1.signature.len(), 128);
    assert_eq!(r1.payload_digest.len(), 64);
    let r2 = signer.sign(payload.to_vec(), 0).expect("sign again");
    assert_eq!(r1.signature, r2.signature, "RFC6979 deterministic");
    let r3 = signer.sign(b"other".to_vec(), 0).expect("sign other");
    assert_ne!(r1.signature, r3.signature);

    // 6. lock: everything refuses again
    let purge = signer.lock().expect("lock");
    assert!(purge.vault_locked);
    assert!(matches!(signer.address(0), Err(CoreError::VaultLocked)));
}

#[test]
fn restore_from_words_yields_same_identity() {
    let (s1, _j1) = SignerHandle::spawn().expect("s1");
    let (s2, _j2) = SignerHandle::spawn().expect("s2");

    let receipt = s1.generate(WordCount::Twelve).expect("generate");
    let phrase = receipt.mnemonic.to_string();

    s1.unlock(Zeroizing::new(phrase.clone())).expect("unlock 1");
    s2.unlock(Zeroizing::new(phrase.clone())).expect("unlock 2");

    let a1 = s1.address(0).expect("a1");
    let a2 = s2.address(0).expect("a2");
    assert_eq!(a1, a2, "same words must restore the same wallet");
    assert_eq!(s1.status().expect("st1"), s2.status().expect("st2"));
}

#[test]
fn different_wallets_are_independent() {
    let (s1, _j1) = SignerHandle::spawn().expect("s1");
    let (s2, _j2) = SignerHandle::spawn().expect("s2");

    let r1 = s1.generate(WordCount::Twelve).expect("r1");
    let r2 = s2.generate(WordCount::Twelve).expect("r2");
    assert_ne!(r1.fingerprint, r2.fingerprint);

    s1.unlock(Zeroizing::new(r1.mnemonic.clone())).expect("u1");
    s2.unlock(Zeroizing::new(r2.mnemonic.clone())).expect("u2");
    assert_ne!(s1.address(0).expect("a1"), s2.address(0).expect("a2"));
}
