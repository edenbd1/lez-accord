# Testnet run: every transaction on the explorer

## `evidence/testnet/public.tsv`

Program header: [`9RgveUGvnm91JPJM8a8mPkScxenQrDNmjzagj8EMv6vr`](https://explorer.testnet.lez.logos.co/account/9RgveUGvnm91JPJM8a8mPkScxenQrDNmjzagj8EMv6vr), ImageID `24d955ee85544249ebe79f3c9bab5482158898ef1753c655ebd59d030e27762a`

| step | expected | verdict | block | transaction |
|---|---|---|---:|---|
| create | applied | applied | 10078 | [`0a3b0fc33619050e…`](https://explorer.testnet.lez.logos.co/transaction/0a3b0fc33619050ebbf2e5220ea34aae3f02736fad92e685e9acb00d95249c1c) |
| queue-before-activation | refused | reverted | 10090 | [`085808126c136be9…`](https://explorer.testnet.lez.logos.co/transaction/085808126c136be9865180c6783fb27903a4a0b8cf83088f21a8863cc2bf107a) |
| spend-before-activation | refused | reverted | 10091 | [`3ae9d067e857b148…`](https://explorer.testnet.lez.logos.co/transaction/3ae9d067e857b148b7f92f3b12b2524d093582f4cb92afeccec9d677535965f1) |
| activate-below-threshold | refused | reverted | 10097 | [`c1b66fe107c73314…`](https://explorer.testnet.lez.logos.co/transaction/c1b66fe107c73314ca4d859387cf859ea67fb772ecd13f02db46844eea5e085c) |
| activate | applied | applied | 10098 | [`967252b5c62d63ea…`](https://explorer.testnet.lez.logos.co/transaction/967252b5c62d63ea221d869aac3c5ac6d8ac288e9f40726cb239459433393c0e) |
| queue-one-approval | refused | reverted | 10101 | [`8766c85cd6adf448…`](https://explorer.testnet.lez.logos.co/transaction/8766c85cd6adf4486f66492c35b7ead4ae618e2b62c514c4fff3cc6b87851d52) |
| queue | applied | applied | 10102 | [`00e8c14b51803f18…`](https://explorer.testnet.lez.logos.co/transaction/00e8c14b51803f18504fa0d7082582d094cb80a27591d916e5f32831edf117d9) |
| execute-in-time-lock | refused | reverted | 10108 | [`8f6df7a4dce8756a…`](https://explorer.testnet.lez.logos.co/transaction/8f6df7a4dce8756ab7a6eb4f4b42d3c1ffd854b8fefe46c2f883d4857faa49b7) |
| execute-by-non-executor | refused | reverted | 10111 | [`359cc3f899aec0f0…`](https://explorer.testnet.lez.logos.co/transaction/359cc3f899aec0f0fd1f29e8a156bd232eb4968a9cd19134098f0f2f85755538) |
| spend-60 | applied | applied | 10112 | [`90f8dedeae43f42d…`](https://explorer.testnet.lez.logos.co/transaction/90f8dedeae43f42d66d377456114d502368224a041d233aead4fee1ef6ffd63e) |
| spend-over-limit | refused | reverted | 10113 | [`b8f752e05b52cbc0…`](https://explorer.testnet.lez.logos.co/transaction/b8f752e05b52cbc0d804eea6f615eaaff1af714645164357414cfb3125a3b289) |
| spend-by-non-spender | refused | reverted | 10114 | [`56443f1155dc397f…`](https://explorer.testnet.lez.logos.co/transaction/56443f1155dc397f86d7085fa21e20cf02ecd3aac2e41789f9ec21c4fa110039) |
| queue-steal | applied | applied | 10120 | [`cd1f667a6e360103…`](https://explorer.testnet.lez.logos.co/transaction/cd1f667a6e360103a006bf87432fae22578defc2fe604f2de6aaaff5b37a561d) |
| queue-config-one-approval | refused | reverted | 10133 | [`059cd564b0a6d107…`](https://explorer.testnet.lez.logos.co/transaction/059cd564b0a6d1074ced1bd4d28003e2991cfddecef49f3e1e460fd4133ea42e) |
| queue-config | applied | applied | 10145 | [`7cbedbe6508e7ab2…`](https://explorer.testnet.lez.logos.co/transaction/7cbedbe6508e7ab25d3c7685aa2d9b8013a6281470612124c9e69841a164ec22) |
| execute | applied | applied | 10186 | [`1d58995c6f83e13e…`](https://explorer.testnet.lez.logos.co/transaction/1d58995c6f83e13ee9cf865f62ef8882ece54b834bf46085b58a3f00f82a4d0e) |
| queue-again | refused | reverted | 10226 | [`a127803d1fba4bfd…`](https://explorer.testnet.lez.logos.co/transaction/a127803d1fba4bfd1430a24fe38592b072c8a262d4967ba2be1aa902d5a91f7b) |
| execute-again | refused | reverted | 10237 | [`49816c71509e82b0…`](https://explorer.testnet.lez.logos.co/transaction/49816c71509e82b0b505e6046092acfe3f70dd51eef3534ea684f0b322b8968f) |
| execute-steal | refused | rejected | None | `-` |
| execute-config | applied | applied | 10249 | [`50eddb4247fe6148…`](https://explorer.testnet.lez.logos.co/transaction/50eddb4247fe6148ebcd5acfc04b0f2d26c85436100e76a88daf607d0411d19b) |
| queue-stale-approvals | refused | reverted | 10262 | [`593a29230a5b9b12…`](https://explorer.testnet.lez.logos.co/transaction/593a29230a5b9b12dacba6922c3f8a1ee2fdd1c2e8eaf810202b1ec5450a6394) |
| queue-removed-member | refused | reverted | 10274 | [`43578e5c3bd1d4e9…`](https://explorer.testnet.lez.logos.co/transaction/43578e5c3bd1d4e97fab32b171e40df5b024ee9edfb7535bc5d66c100209367f) |
| queue-big | applied | applied | 10281 | [`7fb479d6fda11271…`](https://explorer.testnet.lez.logos.co/transaction/7fb479d6fda11271d217bcdf5cd977a1605c1bf918ff200e6751b15fe59935ff) |
| queue-veto | applied | applied | 10293 | [`14193ce7ad48511f…`](https://explorer.testnet.lez.logos.co/transaction/14193ce7ad48511f78bcc0e22e25e0df4ebb66199fdddc5947f7d59176cfc757) |
| execute-veto | applied | applied | 10306 | [`c8cb8d69b56799f2…`](https://explorer.testnet.lez.logos.co/transaction/c8cb8d69b56799f2dce27d27b03a3816080a3ec1526fa967f2f31ec3aca3f516) |
| execute-vetoed | refused | reverted | 10326 | [`1c47d06c3653e6e6…`](https://explorer.testnet.lez.logos.co/transaction/1c47d06c3653e6e63b9861996eecdb7fbceeb545ba6d0bc82b58ff61e7e54ebd) |
| register | applied | applied | 10327 | [`e4b5cb373469cae0…`](https://explorer.testnet.lez.logos.co/transaction/e4b5cb373469cae03e0289f5e338bca9c2f95763bbec024783285f67f9ee1457) |
| register-again | refused | reverted | 10333 | [`996f19c0c89b684b…`](https://explorer.testnet.lez.logos.co/transaction/996f19c0c89b684b8cea5d9302928186d02f8c8ef68def9a1a003acb3dbc84b5) |

## `evidence/testnet/private.tsv`

Program header: [`9RgveUGvnm91JPJM8a8mPkScxenQrDNmjzagj8EMv6vr`](https://explorer.testnet.lez.logos.co/account/9RgveUGvnm91JPJM8a8mPkScxenQrDNmjzagj8EMv6vr), ImageID `24d955ee85544249ebe79f3c9bab5482158898ef1753c655ebd59d030e27762a`

| step | expected | verdict | block | transaction |
|---|---|---|---:|---|
| create | applied | applied | 10434 | [`a9f2a34b6a6f14d7…`](https://explorer.testnet.lez.logos.co/transaction/a9f2a34b6a6f14d729e75693ec57049f1d0ab0f80cbd485e386e61d59d68ed78) |
| activate | applied | applied | 10441 | [`3367e64f4a74130f…`](https://explorer.testnet.lez.logos.co/transaction/3367e64f4a74130fe6341e431935a538e90e848d5416b004b4e77113983f5a3f) |
| fund-private-vault | applied | applied | 10465 | [`727f8318fdaa82f2…`](https://explorer.testnet.lez.logos.co/transaction/727f8318fdaa82f2d370d000bdbe345c58d5c88569fa04268e071f0de838e20c) |
| steal-with-group-keys | refused | rejected | None | `-` |
| spend-private | applied | applied | 10479 | [`b699f403ac42e3b9…`](https://explorer.testnet.lez.logos.co/transaction/b699f403ac42e3b94d77664f33895a9dd6c756b301597b2e778228e8284e0c53) |
| queue | applied | applied | 10481 | [`2289d5c6f62c4037…`](https://explorer.testnet.lez.logos.co/transaction/2289d5c6f62c403706bc0766e7bae1886ede8391dbcb8a3de2bd69db9cd05dbe) |
| execute-private | applied | applied | 10491 | [`9bdd33ee0d6caaab…`](https://explorer.testnet.lez.logos.co/transaction/9bdd33ee0d6caaaba8a8eabbb7d5569e101a704ac7e5881463e959569b8580ac) |

