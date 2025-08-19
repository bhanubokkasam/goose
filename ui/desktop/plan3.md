* we are improving the compaction and summarization ux in this app
* review the diff to date by running: git diff main
* i want to make some improvements now
* 1) when the compaction is in progress do not show a marker. instead simply make the existing /Users/alexhancock/Development/goose/ui/desktop/src/components/LoadingGoose.tsx component show up with the message "goose is compacting the conversation..."
* 2) make it so you can still scroll up and see past messages after compaction is done
* 3) include a marker in the conversation saying compaction occurred at that point and the conversation was summarized
* 4) make the marker UI simpler - just left aligned text instead of having the bars to the right and left of the text
* 5) fix a small visual issue where whenever the markers show up horizontal scroll bars have been introduced
* after making all changes, make sure npm run typecheck passes
