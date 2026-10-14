# Testnet run: every transaction on the explorer

## `evidence/testnet/public.tsv`

Program header: [`CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF`](https://explorer.testnet.lez.logos.co/account/CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF), ImageID `24a69044e34c2604e1a277dc1958855f0a2d95d65d4b0735052b34a51d323cd9`

| step | expected | verdict | block | transaction |
|---|---|---|---:|---|
| create | applied | applied | 10878 | [`7cc35216de661547…`](https://explorer.testnet.lez.logos.co/transaction/7cc35216de66154734778fa42f698aca8927965383853046e5ac573c5ffc0e54) |
| queue-before-activation | refused | reverted | 10901 | [`c6d08abbf825c6e3…`](https://explorer.testnet.lez.logos.co/transaction/c6d08abbf825c6e329e994988e4a55875be6d7a78ac86eaf0cdab9aae7575fe8) |
| spend-before-activation | refused | reverted | 10901 | [`75f8ac23aa986ea9…`](https://explorer.testnet.lez.logos.co/transaction/75f8ac23aa986ea96004658316a2c47798a4aae5c3630451cdedc49f091ed432) |
| activate-below-threshold | refused | reverted | 10901 | [`9e3a90ccb2b20a5b…`](https://explorer.testnet.lez.logos.co/transaction/9e3a90ccb2b20a5b297d72604c72ff87abf1498c5ee7e824f90e6522b775f2f9) |
| activate | applied | applied | 10906 | [`fa64008750f3ebf3…`](https://explorer.testnet.lez.logos.co/transaction/fa64008750f3ebf3be8cd8a1b26fb1efd39cff0ffddefa10a915aa38ed224a0f) |
| queue-one-approval | refused | reverted | 10912 | [`fd95b57411c3cc1e…`](https://explorer.testnet.lez.logos.co/transaction/fd95b57411c3cc1e53d218ff5f3f129a59e5fee65b9a51774966716674a97b64) |
| queue | applied | applied | 10918 | [`df1e0c742576e25d…`](https://explorer.testnet.lez.logos.co/transaction/df1e0c742576e25ddf9e1900a986612ed30f87f79105f0f50c00141c89ade151) |
| execute-in-time-lock | refused | reverted | 10922 | [`0e8471fa725b2684…`](https://explorer.testnet.lez.logos.co/transaction/0e8471fa725b26840f8e771391025619e652692caee75d9b551e94ac20fb1e95) |
| execute-by-non-executor | refused | reverted | 10924 | [`13d5f227998edd44…`](https://explorer.testnet.lez.logos.co/transaction/13d5f227998edd448f496898d8d133d759dc1235429aee9e1ebce814ad2dd557) |
| spend-60 | applied | applied | 10930 | [`a93d331f3f498e63…`](https://explorer.testnet.lez.logos.co/transaction/a93d331f3f498e638a665132d2e6529a0e3d40b1c70d9312172558037bbdb13c) |
| spend-over-limit | refused | reverted | 10931 | [`bbba66b414ad2a14…`](https://explorer.testnet.lez.logos.co/transaction/bbba66b414ad2a14e5ce5c60e9de0aebf6d7e6159383089506889b5eae9797d8) |
| spend-by-non-spender | refused | reverted | 10937 | [`c1cf115515d836f5…`](https://explorer.testnet.lez.logos.co/transaction/c1cf115515d836f5f1d0aac07fb9b051eade91f730d5af931302d5676306faf0) |
| queue-steal | applied | applied | 10939 | [`b261b7cefd918130…`](https://explorer.testnet.lez.logos.co/transaction/b261b7cefd9181300aea219ff48420d08d402af92c1b324c04906e73e635fa62) |
| queue-config-one-approval | refused | reverted | 10943 | [`90dda374b563b44e…`](https://explorer.testnet.lez.logos.co/transaction/90dda374b563b44e8df0774b88c9b37b7b2ed8cfe4f0661e8df48b76303f9ed7) |
| queue-config | applied | applied | 10949 | [`0f45077bd34e460f…`](https://explorer.testnet.lez.logos.co/transaction/0f45077bd34e460fc195c7bdda4181450c2c8094436bdb8ed97571b6291d933d) |
| execute | applied | applied | 10977 | [`c4747aade485dad4…`](https://explorer.testnet.lez.logos.co/transaction/c4747aade485dad42dbaa9d4e1a0598021ae5d9de24ff8ab23b015434c291291) |
| queue-again | refused | reverted | 10977 | [`916e369a73c2fada…`](https://explorer.testnet.lez.logos.co/transaction/916e369a73c2fadac64b30d84f2f49026b81cc19e6d9adc005bcf85758e42101) |
| execute-again | refused | reverted | 10977 | [`4c75ff17faf25846…`](https://explorer.testnet.lez.logos.co/transaction/4c75ff17faf25846d00db6c72e34af68b66f7d18ce2bfd5434b6fa41d7495da8) |
| execute-steal | refused | rejected | None | `-` |
| execute-config | applied | applied | 10977 | [`8dce20e217021848…`](https://explorer.testnet.lez.logos.co/transaction/8dce20e2170218481c96c7ddb24f022024b3e9f7f2f93483e316b6630c9ab8b8) |
| queue-stale-approvals | refused | reverted | 10981 | [`bba8d25fa96643f4…`](https://explorer.testnet.lez.logos.co/transaction/bba8d25fa96643f4be648841c471fb2c6123804d59fa2e7e476d40983a7af49f) |
| queue-removed-member | refused | not-included | None | `d159bd78383a65f37e299d0cf434f2ca09d769f447efc97352bf83725415f344` |
| queue-big | applied | applied | 10990 | [`4f47e0dd6fe8330b…`](https://explorer.testnet.lez.logos.co/transaction/4f47e0dd6fe8330b552b2b619fcce66353903c2af016b90781b2895517831f61) |
| queue-veto | applied | applied | 10999 | [`c6b8269ce35b8fda…`](https://explorer.testnet.lez.logos.co/transaction/c6b8269ce35b8fdafd946e55244577470ea23128903dcf3eee7a6cfaff721b1e) |
| execute-veto | applied | applied | 11005 | [`0ce9f0ffa03e31bb…`](https://explorer.testnet.lez.logos.co/transaction/0ce9f0ffa03e31bb12670fe83460c1d91b05ef9901ba40bd6b086f785c4eede2) |
| execute-vetoed | refused | reverted | 11017 | [`57922e1188405332…`](https://explorer.testnet.lez.logos.co/transaction/57922e11884053329499eef11ec7e99544238d7ac743a8c109a3998c006868cc) |
| register | applied | applied | 11031 | [`b281312fd31906ce…`](https://explorer.testnet.lez.logos.co/transaction/b281312fd31906ce08a74b30abd9a3b113abf18378fe48b6ae804529c181c92a) |
| register-again | refused | reverted | 11037 | [`9ff9808d0149687c…`](https://explorer.testnet.lez.logos.co/transaction/9ff9808d0149687ce99aaf5b9a99bef793dca6e516b023d7df212d7c94203b7f) |

## `evidence/testnet/private.tsv`

Program header: [`CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF`](https://explorer.testnet.lez.logos.co/account/CehtqLan7giiHfVV5qDD9dtXvVi2h7GMdXM87HD2bbQF), ImageID `24a69044e34c2604e1a277dc1958855f0a2d95d65d4b0735052b34a51d323cd9`

| step | expected | verdict | block | transaction |
|---|---|---|---:|---|
| create | applied | applied | 11370 | [`11a85c5ca94948c1…`](https://explorer.testnet.lez.logos.co/transaction/11a85c5ca94948c1b1df7b3c5dbbedd906a99308c86a44d7df7ee4ff7d62de0b) |
| activate | applied | applied | 11400 | [`2876d549fb74c1df…`](https://explorer.testnet.lez.logos.co/transaction/2876d549fb74c1df5f67da0fd8771d7cd4bba6a234d559c230533d2384ef33b7) |
| fund-private-vault | applied | applied | 11424 | [`9f5d10e6ac186082…`](https://explorer.testnet.lez.logos.co/transaction/9f5d10e6ac1860829c9d6efb5ad8a21ee80694cea992277a43cec849251c22d8) |
| steal-with-group-keys | refused | rejected | None | `-` |
| spend-private | applied | applied | 11438 | [`4506c74dd0fc093f…`](https://explorer.testnet.lez.logos.co/transaction/4506c74dd0fc093f5db5c67c21aae34f1ae3700af2ee77661076b8e8bf7ec21c) |
| queue | applied | applied | 11444 | [`3aefdc96f130cfdd…`](https://explorer.testnet.lez.logos.co/transaction/3aefdc96f130cfdd1c2880a6fd4f004ead1bb1bc3b72858fa6918a4981ffd406) |
| execute-private | applied | applied | 11465 | [`eb399bc1c6160e8d…`](https://explorer.testnet.lez.logos.co/transaction/eb399bc1c6160e8d0c24e3809fbe0b1661917660b6f00c381871108239ffb534) |

