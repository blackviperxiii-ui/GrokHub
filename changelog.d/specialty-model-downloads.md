### Added
- Download models can fetch a model made of several files, such as a speech model's encoder, decoder and token list, checking every file against its pinned hash (a small file outside LFS is checked by its git oid). Voice and image models download into folders of their own, so they never replace the chat model you run or show up in the chat model picker.
