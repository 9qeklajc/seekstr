run:
    # cargo run --features whisper -- -b whisper path ~/syncthing/inbox/ASR/
    cargo run -- -b whisper path ~/syncthing/inbox/ASR/

media:
    # Run eventflow router with mediaprocessor

listen:
    nak req --stream wss://seekstr.otrta.me
