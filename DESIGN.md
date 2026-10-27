# Interface system

## Authority

The current direction is the user's October 27 refinement: a concise shadcn SaaS interface, a continuous application shell, settings in columns, and minimal descriptive copy. It supersedes the earlier separate settings page and green palette. Preserve dictionary search, resource management, groups, maintenance, security, and reading behavior.

## Visual language

Operate for search and settings, Read for entries. Use familiar shadcn/Radix controls, Geist with system Chinese fallbacks, lucide icons, neutral surfaces, subtle separators, and compact typography. No decorative illustrations, marketing copy, repeated navigation, or article cards.

## Tokens

- Light: background #ffffff; text #18181b; secondary text #71717a; muted surface #f4f4f5; border #e4e4e7; primary #18181b.
- Dark: background #09090b; text #fafafa; secondary text #a1a1aa; muted surface #18181b; border #27272a; primary #fafafa.
- UI text 14px; supporting text 12px; section titles 18px; panel titles 24px (22px on phones); search 17px.
- Search candidate words and history words share the same `candidate-word` typography: 18px on desktop, 17px on narrow screens, weight 450, and identical line height and letter spacing.
- Controls use the shared shadcn variants and 8–10px radii. Desktop controls are at least 36px tall; mobile touch controls are at least 44px.

## Workspace

A shared left navigation stays mounted across search and settings. Desktop width is 208px, collapsible to a 64px icon rail; each item pairs a lucide icon with a short label and darkens its background on hover. Icons keep their horizontal position throughout collapse and expansion; never switch their alignment to center during the width transition. Settings categories expand below 设置 in this same navigation. At 760px and below, a 60px header opens a modal drawer from the left, with backdrop dismissal, Escape, and focus restoration. Switching views retains the current query, reading position, and pending edits. There is no separate settings header or return-to-search page.

Connection failures use one shared notification with retry and connection-settings actions. Do not repeat the connection error, retry button, or address form inside every settings category. Keep loaded settings visible after a failed refresh; connection fields remain accessible in 连接与安全 even when the service is unavailable. Keep operation-specific errors beside their actions.

Settings use the shared navigation and a content column. Labels sit beside controls on desktop and above them on narrow screens; switches remain beside labels. Settings, dictionary configuration, and groups save automatically. Switches and choices apply immediately; text edits debounce for 400ms. Save requests remain ordered, pending edits survive editor navigation, and failures provide a retry action. There are no save or undo bars.

Dictionary management retains a list next to the selected dictionary details on desktop. On narrow screens the list sits above the details in the same panel. An 添加词典 dropdown offers file import, folder scanning, and authorized server-directory scanning. Import previews appear in the detail pane after a source is chosen. Configuration / information tabs expose resource management and metadata. Resource files have separate MDD, CSS, font, JS, image, audio, and other tabs, each with its own upload filter. CSS files are the primary style source; supplemental inline CSS is collapsed inside the CSS tab. Font selection belongs to the font tab. Resource preview, path editing, ordering, and unbinding live in a row menu; ordering changes only the current file type. Icon selection is one field with a custom path input only when selected.

The group tab has an icon-only plus button labelled 创建分组. It opens a compact name dialog with Enter submission, cancellation, and focus restoration after closing. Do not show 分组操作 or a repeated 分组管理 title. Existing groups remain visible beneath the dialog.

Full-text indexes and cache maintenance live in the dictionary management tabs, never a separate settings category. Each dictionary row pairs a status badge with build/update and a secondary clear action. Task history is collapsed; progress and failures remain visible beside the relevant dictionary. Legacy index links redirect to the dictionary index tab. General performance limits have their own settings category. Index, script, and task states use shared compact badges. Keep one separator between script settings and dictionary removal.

## Copy

Prefer short labels and direct actions. Remove category introductions, obvious field explanations, repeated synchronization text, and implementation details. Keep necessary input semantics, errors, and destructive-action or script-authorization confirmations. Detailed dictionary metadata and source content remain available.

## Search and reading

The normal entry workspace fills the content area beside the navigation instead of centering a narrow reading column. Align the entry search field, dictionary headings, and article frames on the same left edge, with 32px desktop gutters, 20px tablet gutters, and 16px phone gutters. Only explicit reading mode applies the configured content width and centers the column. The empty search home retains its compact search field, horizontally centered in the content area and placed at 40% of the available vertical travel, slightly above the viewport centre. Sidebar width, content margin, and fixed search position animate together.

The same mounted search field, its border, controls, text, and native caret move together from its resting position to the bottom when search is focused. The home resting position is near the centre; entry search rests below the header. Animate the entire container with a position-only transform; never clone or remount the input. Candidates expand above the bottom dock. Selecting a word returns the same field to the top and shows the article, keeping focus/composition behavior coherent. Respect reduced motion, the visual viewport keyboard, and safe areas. The input has no conversational placeholder. Query groups and 词头 use the same shadcn DropdownMenu with borderless ghost triggers, a darker hover/open background, and a quiet keyboard focus outline. The search field has no outer focus glow. Keep removed hit counts and shortcut footnotes absent.

Article text selections clear when clicking outside their frame or interacting with another article. The isolated renderer uses source-checked messages; it never gains same-origin privileges.

Dark theme uses the self-hosted Dark Reader dynamic engine inside each isolated article. Analyze stylesheet and inline foreground, background, border and SVG colors to retain recognizable hues and readable contrast; never invert the entire article. Load the runtime only when a dark article needs it. The trusted parent supplies only bounded CSS/image resources within that article's capability, with shared caching; retain the iframe sandbox and its restrictive connection policy. Match the frame and source color schemes to prevent an opaque browser canvas. Set the initial theme before paint, then update through the trusted bridge without reloading the article. Disabling the engine restores the source colors in light mode.

Keep the search candidate panel mounted and hidden/inert while closed. Its position follows the same moving search container without a separate entrance or exit. Prepare candidates for the current input before focus, preserve matching cached results during background refreshes, and reserve loading placeholders for queries without results.

Headword lookup starts immediately on committed input, with no typing debounce; full-text search retains a 130ms debounce. Reuse cached queries on backspace, and provisionally filter the previous headword list to matches for the current normalized input while its request runs. Never reuse candidates across groups or search modes, never cache a partial list as a complete response, and cancel obsolete requests. Delay loading skeletons by 150ms so quick local lookups do not flash a loader. IME composition does not dispatch requests or submit a word before it is committed.

Returning home moves the search field back near the centre, keeps the same input mounted and transitions the fixed search shell's width and horizontal padding over the same 240ms ease-out as its vertical movement. Do not scale the input text or caret. Navigation resets search state before paint; disabled motion and reduced motion settle immediately.

Dictionary disclosure keeps an already opened article frame mounted for the current word, hidden and inaccessible when collapsed. Reopening preserves its measured height and avoids reloading the document. Never-opened dictionaries remain lazy. Do not animate the article container height; its content is measured asynchronously. Hidden frames cannot change the reader's height or navigate it.

## Responsive and accessibility

Verify desktop and narrow layouts. Keep focus visible, icons labelled, forms keyboard-accessible, IME-safe search, and no horizontal page overflow. Dictionary article frames retain their own typography and one outer reading scrollbar. Light, dark, system theme, error/loading/empty states, and necessary confirmations remain supported.

Use custom neutral scroll thumbs instead of platform scrollbars. The document thumb controls the existing window scroll so anchor navigation and reading-position restoration remain intact; it supports dragging, track clicks, and keyboard navigation. Internal search, sidebar, import, and resource-preview lists use Radix ScrollArea. Thumb appearance follows the active theme and darkens on hover.
