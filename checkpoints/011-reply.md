# Checkpoint 011: reply to a message (local) + WebP preview + shorter picture result

Branch `feat/image-providers`, frozen as `checkpoint/reply`.

- Right-click a bubble, or Ctrl+R (replies to the newest message): a strip "Reply (local, not sent): <preview>" shows above the input. Esc or a click on the strip clears it. The sent bubble shows the quote line above the user's own text.
- Local use of the reply: replying to a message and typing "copy" copies that quoted message. Nothing is sent to the model or online.
- Any other text is sent as typed. The quoted message is NOT added to what goes to the model or online, and the confirm card shows only the typed text. It is a label, not hidden context. Sending quoted text online would need its own preview and confirm (not built).
- Picture result line is now "Picture ready (<provider>)" + saved path + thumbnail. The provider disclosure stays on the confirm card.
- WebP (image-webp 0.2, pure Rust, MIT/Apache) is previewed too; animated WebP is skipped.

Proof: sandbox headless sway, release build /tmp/plreply: Ctrl+R strip, right-click strip (virtual pointer), quote bubble, "copy" local action, online confirm card showed only the typed question. Tests: workspace all pass.
Not proven: Dell/niri, real WebP from Horde, reply to a thumbnail bubble, model-assisted follow-ups using the quote.
