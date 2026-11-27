# Patched copy of i-slint-backend-android-activity 1.19.1

Changes from the crates.io release (java/SlintAndroidJavaHelper.java, javahelper.rs):

- Java numbers each text/selection update it sends to Rust (`seq`); Rust sends the
  last number it handled back with `set_imm_data`. Java ignores replies older than its
  latest update, which used to overwrite newer keystrokes and drop the composing span
  (fast typing came out as e.g. "https://sic.uk.muk.mu").
- A composing span the caret is not inside (Samsung Keyboard re-composes an earlier
  word after a dot) is sent to Slint as plain text: Slint can only place a preedit at
  the caret, so it moved the word there. The keyboard is told its own composing span,
  not Slint's empty preedit at the caret.
- When Slint does change the text, the keyboard's composing span is put back on
  Slint's preedit range.
- The preedit selection is the caret inside the composition instead of the whole
  word. (The renderer still paints the preedit in the selection colour; the app's
  TextField hides that while composing.)

When updating Slint, check whether upstream fixed this; if not, re-apply on the new version.
