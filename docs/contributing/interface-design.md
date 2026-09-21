# Human-centered interface design

Every QilbeeDB interface must help a person complete a task clearly, confidently,
and accessibly. This is a permanent project requirement for new interfaces and
all revisions, including the corporate console and administrative workflows.

## Design the complete task

Define who is acting, their permissions, the outcome they need, their starting
point, and how they recognize success. Design the path from discovery through
completion and recovery before arranging controls. Use familiar task names and
plain English. Explain consequences before consequential actions and offer a
safe way back. Do not require people to know internal identifiers for routine
browsing when resources can be discovered within their authorized scope.

Make navigation consistent and the current company, project, agent, selection,
and active filters understandable. Use recognizable names with technical IDs
available on demand; do not invent display names that could misidentify data.
When discovery is not implemented by the API, record and implement the missing
capability rather than presenting a raw-ID form as a finished management flow.

## Make the important information easy to read

Give primary results and the next useful action visual priority. Use deliberate
spacing, readable type, concise labels, and consistent placement of related
controls. Follow the official QilbeeDB logo, color, and typography system.
Branding must not compromise legibility or accessibility.

Reveal advanced settings, raw contracts, and audit details when needed instead
of placing them ahead of everyday work. Active restrictions, partial coverage,
permission limits, and uncertain outcomes must remain evident. Distinguish no
matching results, no loaded results, incomplete discovery, and no existing data.
Offer a relevant next action, such as clearing a filter, continuing discovery,
retrying a request, or viewing retained records.

## Make every interaction accessible

Use semantic HTML, visible field labels, meaningful accessible names, logical
heading order, and keyboard-operable controls. Provide visible focus, predictable
focus movement and return, usable target sizes, sufficient contrast, and text or
shape cues alongside color. Support responsive layouts and readable zoom without
clipping actions or forcing page-wide horizontal scrolling. Graphs need a usable
text or list alternative, not only an image label. Respect reduced-motion
preferences and announce important status changes without overwhelming users.

Accessibility is part of implementation and review. State precisely what was
tested; do not claim complete standards compliance from a screenshot or an
automated scan alone.

## Respond immediately and support recovery

Acknowledge actions immediately and show meaningful loading, pending, success,
and error states. Prevent duplicate submission while a request is pending.
Explain the problem and a specific next step without exposing secrets. Preserve
safe form input, selection, and confirmed progress when recovery permits it.

For a lost write response, distinguish an unknown outcome from a rejected
command and retry only under the original idempotency identity. Show revision
conflicts and let the person inspect current data rather than overwriting it.
Clear data when authorization ends or the selected scope changes; late responses
must not restore another scope's data. Never present an old observation as a
confirmed current result after refresh fails.

## Validate the person's entire journey

For each changed workflow, record the intended role and task, fixture or release,
steps, expected outcome, observed evidence, and remaining limitations. Validate:

1. Find the correct area and authorized resource without prior knowledge of IDs.
2. Understand the selected company, project, resource, and active filters.
3. Complete the action and recognize its pending and confirmed outcomes.
4. Understand empty results and incomplete coverage and reach a useful next step.
5. Recover from relevant failures: invalid input, denied or expired access, slow
   requests, lost connections, revision conflicts, and retriable service errors.
6. Navigate away and back, change scope, cancel, retry, and sign out without
   stale data, unintended actions, lost confirmed progress, or unauthorized reuse.
7. Complete the same essential journey on desktop and mobile and with keyboard
   navigation and zoom. Check focus, labels, contrast, reading order, and graph
   alternatives; include assistive-technology checks appropriate to the change.

Use automated regression tests for observable behavior and manual inspection for
reading, layout, and interaction. Screenshots alone do not prove a complete
journey. Test error and recovery paths, not only successful requests. Resolve
failures affecting the changed journey before publishing it. Keep validation
evidence separate from unverified claims of improved productivity or reasoning.

Update the relevant English user documentation when behavior changes. Follow the
existing validated-PR and release workflow. Corporate UI code and private release
artifacts remain outside the public repository.
