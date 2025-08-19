import React from 'react';
import { Message, CompactionMarkerContent } from '../../types/message';

interface CompactionMarkerProps {
  message: Message;
}

export const CompactionMarker: React.FC<CompactionMarkerProps> = ({ message }) => {
  const compactionContent = message.content.find(
    (content) => content.type === 'compactionMarker'
  ) as CompactionMarkerContent | undefined;

  const markerText = compactionContent?.msg || 'Conversation compacted';

  return <div className="text-xs text-gray-400 py-2 text-left">{markerText}</div>;
};
