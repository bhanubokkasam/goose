* I am simplifying the UX around context management for this app
* I have already made some changes so review the diff from main via: git diff main
* Now, I want to make it so nothing shows up related to summarization and compaction in the chat window itself. I want it all centralized in the usage of AlertBox related to this
* Remove /Users/alexhancock/Development/goose/ui/desktop/src/components/context_management/ChatContextManager.tsx and /Users/alexhancock/Development/goose/ui/desktop/src/components/context_management/ContextHandler.tsx and their usages and then make one new version of these things that enables the following UX:
    * No UI in the chat window itself other than a small message that occurs that indicates a summary was prepared to compact the conversation
    * When the limit is reached and compaction occurs for that reason, or the user manually triggers compaction, have the system prepare the summary and automatically send the summary message so the conversation just rolls on
