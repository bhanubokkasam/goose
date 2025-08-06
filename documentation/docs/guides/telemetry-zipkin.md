---
title: Telemetry with Zipkin Support
sidebar_position: 20
sidebar_label: Telemetry & Tracing
---

import Tabs from '@theme/Tabs';
import TabItem from '@theme/TabItem';

Goose logs telemetry data in a Zipkin-compatible JSON format that can be ingested and visualized in distributed tracing systems like Zipkin, Jaeger, or other OpenTelemetry-compatible tools.

## How It Works

When a Goose session starts, the telemetry logger automatically creates two files in the `~/.local/share/goose/sessions/telemetry/` directory:

1. **`{session_id}.jsonl`** - Raw telemetry logs in JSONL format (one JSON object per line)
2. **`{session_id}_zipkin.json`** - Zipkin-compatible spans in JSON array format

## Telemetry Data Captured

The telemetry system captures:

- **LLM API Calls**: Complete and streaming requests to language models
- **Tool Executions**: Start and end times for tool calls
- **Wait Events**: Timing for various operations (LLM waits, tool waits, etc.)
- **API Requests**: HTTP requests made to provider APIs

Each span includes:
- Trace ID (unique per session)
- Span ID (unique per operation)
- Timestamps (in microseconds)
- Duration (when available)
- Tags (provider, model, request type, sizes, errors)
- Service endpoints

## Zipkin Format

The telemetry data follows the [Zipkin v2 API specification](https://zipkin.io/zipkin-api/#/default/post_spans). Each span includes:

```json
{
  "traceId": "unique-trace-id",
  "id": "unique-span-id",
  "name": "operation-name",
  "timestamp": 1234567890,
  "duration": 1500000,
  "kind": "CLIENT",
  "localEndpoint": {
    "serviceName": "goose",
    "ipv4": "127.0.0.1"
  },
  "remoteEndpoint": {
    "serviceName": "provider-name"
  },
  "tags": {
    "provider": "openai",
    "model": "gpt-4",
    "request_type": "complete",
    "request.size": "1024",
    "response.size": "2048"
  }
}
```

## Viewing in Zipkin

To visualize the telemetry data in Zipkin:

1. **Start Zipkin** (using Docker):
   ```bash
   docker run -d -p 9411:9411 openzipkin/zipkin
   ```

2. **Import the telemetry data**:
   ```bash
    curl -X POST http://localhost:9411/api/v2/spans \
      -H "Content-Type: application/json" \
      --data-binary @"$(echo ~/.local/share/goose/sessions/telemetry/{session_id}_zipkin.json)"
   ```

3. **View in browser**: Open http://localhost:9411 and search for your traces

## Viewing in Jaeger

For Jaeger (which also supports Zipkin format):

1. **Start Jaeger** (using Docker):
   ```bash
   docker run -d --name jaeger \
     -p 16686:16686 \
     -p 9411:9411 \
     jaegertracing/all-in-one:latest
   ```

2. **Import the telemetry data** (same as Zipkin):
   ```bash
   curl -X POST http://localhost:9411/api/v2/spans \
     -H "Content-Type: application/json" \
     -d @~/.local/share/goose/sessions/telemetry/{session_id}_zipkin.json
   ```

3. **View in browser**: Open http://localhost:16686

## Example Use Cases

### Analyzing LLM Performance
- Track response times for different models
- Compare streaming vs. complete API calls
- Identify slow providers or models

### Tool Execution Analysis
- Measure tool execution times
- Identify bottlenecks in tool chains
- Track parallel vs. sequential execution patterns

### Session Performance
- View the complete timeline of a Goose session
- Identify wait times and delays
- Optimize interaction patterns

## Implementation Details

The telemetry system:
- Automatically initializes for each session
- Logs data asynchronously (non-blocking)
- Maintains span relationships for paired events (START/END)
- Generates unique trace IDs per session
- Converts timestamps to microseconds (Zipkin standard)
- Includes error information when operations fail
