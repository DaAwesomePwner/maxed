# Decisions

Durable product and engineering decisions that later work must respect. One line each, current state only.

## UI

- Visual language: graphite neutrals with no violet in the chrome; fame gold marks you (your row, figure, line) and the one primary action; other players are neutral steps; bag, rarity, tier, chat-channel and HP colors are the only rich color; no glows, gradients, colour washes or looping motion; one type scale with a 12px minimum and weights 400/500 (600 rare); sentence case; 4px spacing grid; Segoe UI with no webfonts. Why: approved "unslop" direction so the app reads as one deliberate, game-native product instead of generic AI styling; details in docs/architecture/ui.md "Visual language". (2026-11-10, unslop-ui, merge 4956ec9)
- Gold distinguishes you among other players; on screens where every row is you (for example your own damage sources) bars stay neutral. Why: gold on every row stops meaning anything. (2026-11-10, unslop-ui, merge 3fe6565)
- Error copy shows one "Couldn't …" line per failure, paired with its own retry where one is on screen; tabbed screens keep failures in the always-visible top list. Why: chained "Couldn't X. Retry." text was unreadable, and inline messages in hidden tab panels went unseen. (2026-11-10, unslop-ui, merge 4956ec9)
