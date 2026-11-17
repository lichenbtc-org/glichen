# The committed guest

`hp.bin` is the program the prover (`glichen-r0`) proves and BABE's
verifier checks receipts of: the guest's ELF with RISC Zero's kernel
(V1COMPAT), built by `../build-guest.sh` in RISC Zero's pinned builder
image. `hp.id` is its image id (`glichen-r0 image-id hp` recomputes it
from the file; `glichen-r0 check-id` checks both, and the prover image
runs it).

`hp` is the hub's statement, the grants L2's (`glichen_statement::accounts`,
papers/lichen-grants.pdf): "the claim posted in `request` is owed at most
the `b_corr` signed in `correct`"; it holds no other guest's id. BABE
checks its Groth16 wrap (`lichen_babe::statement::Kind::Risc0`,
`IMAGE_ID` this `hp.id`); the hub proves it (`r0/hp-prover.sh hp-proof`).

Its builds, newest first (the guests of the validity rollup this repo
split from are in vlichen):

| guest | built by | image id |
|---|---|---|
| **hp**, the grants L2's statement at v0.5, the same sources under RISC Zero 3.1.0 (builder r0.1.97.0), 2026-11-16: **the current file** | x86 Linux (lichen-vps), `build-guest.sh elf hp`; packed on a Mac (`glichen-r0 pack`, 3.1.0). ELF SHA-256 `f3757c1520778e74d5fbfdc5b7841e873b35fe734ebce214b8c2880926169dc4`; `hp.bin` 366,168 bytes, SHA-256 `d4c48c9c98d7945e843309a8300980ae38ea6b005f7a86b54bc77efd65d5c000`. Executed (`exec-hp`) on the contested test's proved exit (one step cited): 2,463k user cycles, 3 segments; the STARK proved on a Mac (14 cores) in 179 s | **`d825f25c44d20ba52c0d6fe02236efbc6c958ed22bd61c0d14d55c07c16f9be9`** |
| **hp**, the grants L2's statement at v0.5 (papers/lichen-grants.pdf: one note per account, Algorithm 1 over the cited steps, her four Winternitz signatures on SHA-256 chains checked, the chainless journal of 36 bytes), 2026-11-14 | x86 Linux (lichen-vps), `build-guest.sh elf hp`, then `pack hp` there. ELF SHA-256 `bc3e562cb55264c8343536918e575e91ea247579a7f8c3ee95cdc9e370c69742`; `hp.bin` 405,236 bytes, SHA-256 `d81115134e8ed943001a3b83b65f520929c165c1dc184ddfd124f09bf0887218`. Executed (`exec-hp`) on the contested test's proved exit (P: one step cited): 2,681k user cycles, 3 segments. Proved for real: the STARK on a Mac (14 cores) in 179 s, the Groth16 wrap on lichen-vps in 264 s; the seal verifies under BABE's `Kind::Risc0` for its journal | **`0e3809e477c657eb74f1771727f36fc87ed8c88f069da117d4639c2c331dc78e`** |
| **hp**, the grants L2's statement with reveals (milestone 5: a cited spend counts through its reveal completed in a slot of `correct`; no fee child, no other claims), 2026-11-13 | x86 Linux (lichen-vps), `build-guest.sh elf hp`, then `pack hp` there. ELF SHA-256 `c24a0af02012edaea44828080a1abb2d7a29e60e09853f12aa4f1c66b928ec74`; `hp.bin` 438,020 bytes, SHA-256 `43d3517b6979cf4b65f7b90d89f44c0266f9d6933c5b8081ac420f9da876797d`. Executed (`exec-hp`) on the contested test's corrected exit (one spend cited, 12 headers): 1,306k user cycles, 2 segments | **`1b2e907a3956c4fdabc6b89f1cc6f2cc71ebd6687dc81cd9d73c7ef47de597cc`** |
| **hp**, the grants L2's statement hardened (milestone 4: the posted claim one-time signed in full, the hub's signatures half-aggregated, the owner in `D`), 2026-11-13 | x86 Linux (lichen-vps), `build-guest.sh elf hp`, then `pack hp` there. ELF SHA-256 `e66d60d5dacee70cb6ea498dbb4ea04cd4d416cf60ccb4ae57765e0101b4dbd7`; `hp.bin` 470,944 bytes, SHA-256 `9dd4a239252782eeedc8537f695ef6315b81664dc5dcf04bbb5c895405170f31`. Executed (`exec-hp`) on the contested test's corrected exit (one spend cited, 12 headers): 1,456k user cycles, 2 segments | **`efe219a1238528ae730f11ba1b6a8e22732924fa24d08fe7ab80871a2698f1c4`** |
| **hp**, the grants L2's statement (milestone 3), 2026-11-12 | x86 Linux (lichen-vps), `build-guest.sh elf hp`, then `pack hp` there. ELF SHA-256 `b095a5659983381e3afc241fa96a365e65a9e4ac63417c9fad341fc87e115db2`; `hp.bin` 460,144 bytes, SHA-256 `906c210240cf9fe80d80b719986fc92797fe1fd17c35823ed0ff4f3565b0e81b`. Executed (`exec-hp`) on the contested test's corrected exit (`LICHEN_DUMP_HP`: one spend cited, 12 headers): 1,281k user cycles, 2 segments | **`fc24dc37666c562c38912ac8c5afb168b61883f28f94bedcba3c4b5efa163c59`** |
