import React, { createContext, useContext, useState, useCallback } from 'react';
import { Message } from '../../types/message';
import { manageContextFromBackend, convertApiMessageToFrontendMessage } from './index';

// Define the context management interface
interface ContextManagerState {
  isCompacting: boolean;
  compactionError: string | null;
}

interface ContextManagerActions {
  handleAutoCompaction: (
    messages: Message[],
    setMessages: (messages: Message[]) => void,
    append: (message: Message) => void
  ) => Promise<void>;
  handleManualCompaction: (
    messages: Message[],
    setMessages: (messages: Message[]) => void
  ) => Promise<void>;
  hasCompactionMarker: (message: Message) => boolean;
}

// Create the context
const ContextManagerContext = createContext<
  (ContextManagerState & ContextManagerActions) | undefined
>(undefined);

// Create the provider component
export const ContextManagerProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [isCompacting, setIsCompacting] = useState<boolean>(false);
  const [compactionError, setCompactionError] = useState<string | null>(null);

  const performCompaction = useCallback(
    async (
      messages: Message[],
      setMessages: (messages: Message[]) => void,
      append: (message: Message) => void,
      isManual: boolean = false
    ) => {
      setIsCompacting(true);
      setCompactionError(null);

      try {
        // Add a compaction marker message to show in the chat
        const compactionMarker: Message = {
          id: `compaction-marker-${Date.now()}`,
          role: 'assistant',
          created: Math.floor(Date.now() / 1000),
          content: [
            {
              type: 'compactionMarker',
              msg: isManual
                ? 'Compacting conversation as requested...'
                : 'Context limit reached. Compacting conversation...',
            },
          ],
          display: true,
          sendToLLM: false,
        };

        // Add the marker to the messages
        setMessages([...messages, compactionMarker]);

        // Get the summary from the backend
        const summaryResponse = await manageContextFromBackend({
          messages: messages,
          manageAction: 'summarize',
        });

        // Convert API messages to frontend messages
        const convertedMessages = summaryResponse.messages.map(
          (apiMessage) => convertApiMessageToFrontendMessage(apiMessage, false, true) // don't show to user but send to llm
        );

        // Extract summary from the first message
        const summaryMessage = convertedMessages[0];
        if (
          summaryMessage &&
          summaryMessage.content[0] &&
          summaryMessage.content[0].type === 'text'
        ) {
          // Update the compaction marker to show completion
          const completedMarker: Message = {
            ...compactionMarker,
            content: [
              {
                type: 'compactionMarker',
                msg: 'Conversation compacted. Summary prepared to continue.',
              },
            ],
          };

          // Replace messages with just the completed marker and summary
          setMessages([completedMarker, summaryMessage]);

          // Automatically submit the summary message to continue the conversation
          setTimeout(() => {
            append(summaryMessage);
          }, 100);
        }

        setIsCompacting(false);
      } catch (err) {
        console.error('Error during compaction:', err);
        setCompactionError(err instanceof Error ? err.message : 'Unknown error during compaction');

        // Update the marker to show error
        const errorMarker: Message = {
          id: `compaction-error-${Date.now()}`,
          role: 'assistant',
          created: Math.floor(Date.now() / 1000),
          content: [
            {
              type: 'compactionMarker',
              msg: 'Compaction failed. Please try again or start a new session.',
            },
          ],
          display: true,
          sendToLLM: false,
        };

        setMessages([...messages, errorMarker]);
        setIsCompacting(false);
      }
    },
    []
  );

  const handleAutoCompaction = useCallback(
    async (
      messages: Message[],
      setMessages: (messages: Message[]) => void,
      append: (message: Message) => void
    ) => {
      await performCompaction(messages, setMessages, append, false);
    },
    [performCompaction]
  );

  const handleManualCompaction = useCallback(
    async (messages: Message[], setMessages: (messages: Message[]) => void) => {
      await performCompaction(messages, setMessages, () => {}, true);
    },
    [performCompaction]
  );

  const hasCompactionMarker = useCallback((message: Message): boolean => {
    return message.content.some((content) => content.type === 'compactionMarker');
  }, []);

  const value = {
    // State
    isCompacting,
    compactionError,

    // Actions
    handleAutoCompaction,
    handleManualCompaction,
    hasCompactionMarker,
  };

  return <ContextManagerContext.Provider value={value}>{children}</ContextManagerContext.Provider>;
};

// Create a hook to use the context
export const useContextManager = () => {
  const context = useContext(ContextManagerContext);
  if (context === undefined) {
    throw new Error('useContextManager must be used within a ContextManagerProvider');
  }
  return context;
};
