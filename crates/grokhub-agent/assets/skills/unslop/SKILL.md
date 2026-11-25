---
name: unslop
description: Cut AI tells from any writing.
disable-model-invocation: true
---

# Unslop

Edit text to remove AI patterns. Scan for the patterns below, then rewrite. Preserve meaning and match the intended tone.

Rule numbers are stable ids that other skills cite. A removed rule leaves a gap.

3. No trailing "-ing" phrases that add no fact ("highlighting...", "ensuring...", "showcasing..."). Cut the phrase or give it a fact.
5. No vague attributions ("experts believe"). Name the source or delete.
7. No AI vocabulary: additionally, crucial, delve, enduring, enhance, foster, fostering, garner, interplay, intricate, landscape, tapestry, pivotal, showcase, testament, underscore, vibrant. "Landscape" and "tapestry" are fine for a real landscape or tapestry.
8. Say "is" or "has", not "serves as", "stands as", "boasts", "features".
9. No "not just X, but Y". State the point.
10. Don't force ideas into threes. Use the count the ideas have.
11. One name per thing. Repeat it.
12. No "from X to Y" unless X and Y sit on a real scale. Otherwise list the items.
13. No em dashes, en dashes, or hyphens used as dashes, and no parentheses in their place. Use a period or a comma.
14. A colon goes before a list or an example, never mid-sentence as a connector.
15. Don't bold every proper noun or acronym.
16. No bold label and colon that restates its line ("**Performance:** Performance improved..."). Write it as prose. A bold lead-in that ends in a period and adds new detail is fine.
17. Sentence case headings.
18. No decorative emojis.
19. Straight quotes.
20. No chatbot phrases ("I hope this helps!", "Let me know if...", "Of course!", "Certainly!", "smoking gun").
22. No praise or agreement openers. Respond directly.
23. Cut filler. "In order to" becomes "to". "Due to the fact that" becomes "because". Delete "it is important to note that".
24. One hedge at most: "may".
25. No generic conclusions. State specific plans or facts, or the next step.
26. No abstract metaphor nouns: substrate, wedge, vector, locus, vantage, nexus, primitive, harness, surface, bedrock, scaffolding, modality, paradigm, gold-plating, ratchet, evacuate, endgame, north star, flywheel. Use the concrete word. "Vector" for a way of doing something, "primitive" as a noun, and "evacuate" for moving something are the tells. A word here is fine in its literal sense.
27. Say what it does, not how it feels. Name the mechanism or the number. Cut a sentence that could appear unchanged in another project's docs.
28. One idea per sentence. Split a sentence the reader has to reread.
29. Active voice. Name the actor unless it is unknown or doesn't matter.
30. Cut adverbs or use a stronger verb. Give the number.
31. Use the plain word: "use", not "utilize" or "leverage". "Help", not "facilitate". "Many", not "numerous". "If", not "in the event that".
32. No metaphor, aphorism, personified code, or figurative verb where a literal phrase exists.
33. Write whole sentences with their articles and verbs. Spell out arrows and abbreviations.
