# probbit brand

- The name is one word, lowercase: **probbit** (never "ProbBit", never two words). The probabilistic bit it is built from is a **p-bit** (plural p-bits).

## The mark: the agent rabbit

A white rabbit with a round head and two angular ears. One cyan `#00F0FF` visor runs across the eyes, set in a thin band of the
ground colour. A lightning cut enters the right ear from its tip. That is the whole mark: no outline, no gradient, no text.

At 16 px the lightning reads as a notch in the right ear tip; that is by design. From 32 px up it reads as a bolt.

## Files

| file | what it is for |
|---|---|
| `logo.svg` | the mark, white body, transparent; for dark grounds |
| `logo-ink.svg` | the same geometry with an ink body, transparent; for light grounds (the visor stays cyan) |
| `lockup-probbit.svg` | mark + word, white, glyphs outlined (no font needed), cyan i-dot; for dark grounds |
| `lockup-probbit-ink.svg` | the same lockup with an ink mark and word; for light grounds |
| `lockup-probbit.png` | the lockup on ink, 1200×360 (2x for 600×180) |
| `lockup-probbit-light.png` | the ink lockup on white, 1200×360 |
| `favicon.svg` | browser tab icon: the white mark on an ink square |
| `favicon.ico` | 16, 32 and 48 px in one file, for browsers that ask for `/favicon.ico` |
| `favicon-16.png`, `favicon-32.png` | PNG tab icons |
| `apple-touch-icon-180.png` | iOS home screen (opaque, ink square) |
| `icon-512.png` | app manifests and large avatars (ink square) |

Elsewhere in `docs/`: `probbit-hero-1600x900.jpg` (README header, rendered from `probbit-hero.html` with headless Chrome at
1600×900) and `probbit-social-1280x640.png` (the repository's social preview).

## Colours

- Ink `#0b0e14` (ground, or body on light grounds), white `#ffffff` (body on dark grounds), cyan `#00F0FF` (visor and i-dot).
- Cyan is the one accent. It appears on the visor and the dot of the i, nowhere else in the mark or the lockup.

## Wordmark and lockup

- The word is Avenir Next 700, white on ink with a cyan `#00F0FF` dot on the i; on light grounds the word is ink.
- In the lockup the mark's body is about three quarters of the word's height (top of the b to the foot of the p), centred on it,
  with about half an em between mark and word.

## Clear space and minimum sizes

- Clear space: keep at least the mark's ear width (about a sixth of the mark's height) empty on every side of the mark or the lockup.
- Minimum sizes: mark 24 px tall on screen, word 22 px; below that use the mark alone. Under 24 px, use the favicon files, which are drawn for it.

## Do and don't

- Do use the files as they are: white body on dark grounds, ink body on light grounds, visor always cyan.
- Don't recolour the body or the visor, add a second accent colour, outline, stretch, rotate or add effects.
- Don't move the bolt: it is always a cut in the tip of the right ear, never on the left ear, the head or the word.
- Don't redraw the word in another typeface or put the bolt inside the name.
- The 0.5.0 mark (a rabbit with a banded ear and a double arrow under it) is retired; don't use it anywhere.
