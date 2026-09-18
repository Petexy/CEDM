# LineXinBar shared-surface origin

The following files were copied from LineXinBar (<https://github.com/Petexy/LineXinBar>) and remain under that project's GPL-3.0-only terms:

| CEDM file | LineXinBar origin |
| --- | --- |
| `src/shaders.wgsl` | `crates/lxb-desktop/src/shaders.wgsl` |
| `src/offscreen.wgsl` | `crates/lxb-desktop/src/offscreen.wgsl` |
| `src/visual/theme.rs` | `crates/lxb-desktop/src/theme.rs` |
| `src/steam_hid.rs` | `crates/lxb-desktop/src/steam_hid.rs` |
| `assets/fonts/Roboto-*.ttf`, `assets/fonts/LICENSE.txt` | `font/Roboto/static/*` and `font/Roboto/LICENSE.txt` |
| `assets/glyphs/*` | matching `icons/*.svg` files |
| `assets/sounds/*` | matching `crates/lxb-desktop/src/sounds/*.ogg` files, at commit `0290f37` |

The two Noto faces beside them — `assets/fonts/NotoSansDevanagariUI-{Regular,Bold}.ttf` and `assets/fonts/NotoSansCJKsc-{Regular,Bold}.ttf` — are **not** LineXinBar's. They are subsets of Google's Noto Sans Devanagari UI and Noto Sans CJK SC, cut down to what this login screen draws, and they are here because CEDM says things in ten languages and the shell's Roboto has no Devanagari and no Han in it. They keep their own licence, `assets/fonts/LICENSE-NotoSans.txt` (SIL Open Font License 1.1); neither family declares a Reserved Font Name, so the subsets keep their original names. See "What it says, and in which language" in the README.

`lxb-wallpaper-v2` is the compatibility ABI for the shader, palette uniforms, coordinate system, linear-light colour handling and monotonic scene clock. Bump it in both repositories whenever those pixels or semantics change.

## The four recordings

Copied rather than reimplemented, and that is the whole point of them: signing in and using the shell that follows should be one instrument. A user who has just learnt what a press sounds like on the login screen has learnt what it sounds like everywhere.

Four of LineXinBar's eleven, because a login screen is four of its controls:

| CEDM sound | LineXinBar clip | What it answers there |
| --- | --- | --- |
| the highlight moving | `press-guide.ogg` | a direction that moved something in the Home Button guide |
| a press acted on | `press-selected.ogg` | a press on Start that the shell then keeps |
| an on-screen key going down | `keyboard-click.ogg` | the same board — this greeter runs LineXinBar's keyboard model |
| a password refused | `error.ogg` | a refusal, in the shell's own panels |

The pairing of the first two is deliberately **not** LineXinBar's own. There, `press-guide.ogg` pairs with `press-guide-selected.ogg` and `press.ogg` with `press-selected.ogg`, so the ear can tell the bar from the overlay standing over it. A login screen is one screen and has nothing to be told apart from, so it takes the guide's move — the guide is that shell's short column of controls standing over everything else, which is what this whole screen is — and Start's press, which is the more emphatic of the two and the right weight for a login going through.

`src/sound.rs` is a CEDM-owned implementation of the same shape as `crates/lxb-desktop/src/sound.rs`: the same rodio cut, the same "silence is a working state" handling of a device that is missing or lost, and the same rule that a clip is never laid on top of a copy of itself. It carries no music, and it is spent only from the button path, which that shell does not restrict in the same way.

Where it diverges most is in **which device it opens**, and that is not a stylistic difference. The shell runs inside a session with a sound server, so ALSA's `default` is the whole answer and rodio's own device walk never has to run. A greeter has no sound server, so `default` fails and the walk is the path that actually executes — and it walks ALSA's name hints, which are mostly plugins rather than speakers. `src/sound.rs` therefore does its own selection, and `src/audio.rs` plus two keys in the published look are how it is told the right answer instead of guessing it. See "Which speakers, and how loud" in the README.

That answer is CEDM's own to obtain, and deliberately so: `cedm-session` publishes it behind **any** desktop, once that session has a sound server to be asked. LineXinBar refines it — the shell publishes when the output device is changed and again on its way out, which is the only honest moment to read a volume — but nothing here depends on that shell being the one running. Neither project requires the other: LineXinBar treats a missing `cedm` as the ordinary case and pays nothing for it, including on the way out, and CEDM's login screen sounds work in front of a session that has never heard of LineXinBar.

The two projects also disagree about volume on purpose. LineXinBar's clips are played at whatever its own mixer's System row says, because that row exists and the user set it. CEDM has no such row — there is nobody at a login screen to have set one — so it plays at the gain the *session* was at, which is the nearest true thing to "as loud as this machine is".

## Where `src/visual/theme.rs` deliberately differs

The material state at the end of it — `Part`, `PARTS`, `style`, `set_style`,
`preview_style`, `restore_style` and `style_flag`, which are what
`theme-wallpaper` and `theme-icons` reach — is LineXinBar's own code, and it names
the same two halves and the same two values. What it cannot share is the *type*:
the shell takes `Style` from `lxb-protocol`, the crate it shares with its own
compositor, and this program vendors that scene rather than depending on the
crate. So the enum and its table live in `src/accent.rs`, beside the reader that
pulls the keys out of `shell.toml`, and this file refers to them — `THEMES` for
the shell's `wallpaper::STYLES`, and `accent::style` for `wallpaper::style`.
Anything else in the block is the shell's, verbatim.

`Part` itself is *not* one of those substitutions. It is the shell's enum, copied
whole, and `src/accent.rs` refers back to it here rather than keeping a second
one — which half of a setting is being talked about is not a fact about the
vendored scene.

There is a fourth substitution now, and it is the only one that is a difference
in *behaviour* rather than in spelling: `Part::styles` answers with the same two
materials for the wallpaper as for the marks, where the shell's answers with a
third under the wallpaper — a picture or a film of the user's own. This program
does not draw one and could not: the file is under one account's home directory
and this login screen stands in front of every account on the machine, before any
of them is unlocked. `accent::CUSTOM_WALLPAPER` is where that setting is
recognised, and the shell's own scene is what it is answered with.

## Where `src/shaders.wgsl` deliberately differs

The wallpaper is drawn and evaluated **per display** rather than once across the surface: `vs_background` takes a display rectangle per instance and is drawn once for each of them, `fs_background` asks the wallpaper about the display's own `uv` and aspect, and `behind_at` — the fall-through a pane of glass uses where nothing was drawn behind it — takes the display the pane stands on. The `wallpaper` function itself, the palette uniforms and every constant are untouched.

The `glyph_material` in this copy also reads its field with a three-texel arm
where the shell reads one and a half, because this atlas's cells are twice the
shell's — that is the shell's number expressed in this program's units, and it is
explained where it is written.

Both copies read `globals.style.y` for a mark and `globals.style.x` for the
wallpaper. That is not a divergence either — the Theme setting is two settings
now, and the pair travels in one `vec4` because a uniform block is laid out in
sixteen-byte lots.

`globals.style.x` has a third value, `2`, and it is not a material: the shell's
Wallpaper setting can be a picture or a film of the user's own, which `wallpaper`
returns early for and reads out of `paper_texture` at `@group(3) @binding(2)`.
**This program never writes that value.** The picture is a file under one
account's home directory and this greeter stands in front of every account on the
machine, before any of them is unlocked; `accent::CUSTOM_WALLPAPER` is where that
setting is recognised and answered with the shell's own scene, which is also what
the session's own compositor does for its bridge frame. The binding is a single
transparent texel here, beside the empty scenery that is there for the same
reason: `wallpaper` stays the shell's function verbatim, which is the whole
contract, and the branch it feeds is never taken.

`lxb-wallpaper-v2` is untouched by that. Nothing about the pixels either program
draws for `Default` or `Simple` changed, and a machine set to the third value
hands over exactly the frame it would have handed over before it existed.

That is not a divergence from the ABI, it is what the ABI requires here. LineXinBar has one layer surface per output and each evaluates the wallpaper against its own output's size; CEDM has a single surface that its compositor extends across every output, so it has to do per instance what the shell does per surface. On a machine with one display the two are the same arithmetic and the same pixels. A newer copy of `shaders.wgsl` taken from `lxb-desktop` has to have these three changes reapplied, or the handover grows a seam on every machine with two monitors on it.

The session manager, greetd client, UI composition and wallpaper-clock transport are CEDM-owned implementations.
