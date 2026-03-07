import json
import pandas as pd
import argparse
import sys
from pathlib import Path


def analyze_trace(file_path):
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except FileNotFoundError:
        print(f"Error: File not found: {file_path}", file=sys.stderr)
        return None
    except json.JSONDecodeError:
        print(f"Error: Invalid JSON file: {file_path}", file=sys.stderr)
        return None

    # Extract events
    if isinstance(data, list):
        events = data
    elif isinstance(data, dict):
        events = data.get('traceEvents', [])
    else:
        print("Error: Unknown trace file format (not a list or dict)", file=sys.stderr)
        return None

    if not events:
        print("Error: No traceEvents found", file=sys.stderr)
        return None

    # Filter for relevant events with frame_id
    # We are looking for:
    # 1. frame_processing (Capture start) - ph='B' (Begin) or 'X' (Complete)
    # 2. queue_encode_job (Encoder queue) - ph='B' or 'X'
    # 3. write_sample (Send start) - ph='B' or 'X'

    frame_events = []
    
    # Store process_input/output events to match B/E
    # Key: (tid, event_name), Value: {frame_id, start_ts}
    # Note: frame_id might be in "E" event or "B" event args.
    # We will store based on TID assumption that events are nested properly or sequential
    
    # Process events
    for e in events:
        name = e.get('name')
        ts = e.get('ts', 0) / 1000.0 # Convert to ms
        ph = e.get('ph')
        tid = e.get('tid')
        pid = e.get('pid')
        
        args = e.get('args', {})
        if not args:
            continue
            
        frame_id = None
        
        # Extract frame_id from args if present (string or int)
        if 'frame_id' in args:
            try:
                frame_id = int(args['frame_id'])
            except (ValueError, TypeError):
                pass
        
        # Handle specific events
        if name == 'frame_processing' and ph == 'B':
            # Video capture start
            if frame_id is not None:
                frame_events.append({'frame_id': frame_id, 'event': 'capture_start', 'ts': ts})
                
        elif name == 'queue_encode_job':
            # Frame router queue
            # Note: We added frame_id to this span recently
            if frame_id is not None:
                frame_events.append({'frame_id': frame_id, 'event': 'encode_queue', 'ts': ts})
        
        elif name == 'handle_need_input':
             if frame_id is not None:
                if ph == 'B':
                    frame_events.append({'frame_id': frame_id, 'event': 'handle_input_start', 'ts': ts})
        
        elif name == 'preprocess':
             if frame_id is not None:
                if ph == 'B':
                    frame_events.append({'frame_id': frame_id, 'event': 'preprocess_start', 'ts': ts})
                elif ph == 'E':
                    frame_events.append({'frame_id': frame_id, 'event': 'preprocess_end', 'ts': ts})

        elif name == 'buffer_create':
             if frame_id is not None:
                if ph == 'B':
                    frame_events.append({'frame_id': frame_id, 'event': 'buffer_create_start', 'ts': ts})
                elif ph == 'E':
                    frame_events.append({'frame_id': frame_id, 'event': 'buffer_create_end', 'ts': ts})

        elif name == 'process_input':
            if frame_id is not None:
                if ph == 'B':
                    frame_events.append({'frame_id': frame_id, 'event': 'process_input_start', 'ts': ts})
                elif ph == 'E':
                    frame_events.append({'frame_id': frame_id, 'event': 'process_input_end', 'ts': ts})
            # Handle case where E event might not have args but we want to capture it?
            # Tracing crate adds args to B and usually E.
            # If E misses frame_id, we might miss end time.
            # But we added frame_id to span, so it should be there.

        elif name == 'process_output':
            if frame_id is not None:
                if ph == 'B':
                    frame_events.append({'frame_id': frame_id, 'event': 'process_output_start', 'ts': ts})
                elif ph == 'E':
                    frame_events.append({'frame_id': frame_id, 'event': 'process_output_end', 'ts': ts})

        elif name == 'write_sample':
            # Send start
            if frame_id is not None:
                frame_events.append({'frame_id': frame_id, 'event': 'send_start', 'ts': ts})

    if not frame_events:
        print("No relevant frame events found in trace.", file=sys.stderr)
        return {"frame_count": 0, "stats_cols": [], "valid_frames": pd.DataFrame()}

    df = pd.DataFrame(frame_events)
    
    # Pivot to have one row per frame_id (using 'first' to handle duplicates if any)
    df_pivot = df.pivot_table(index='frame_id', columns='event', values='ts', aggfunc='first')
    

    # Calculate Latencies
    # 1. Capture Latency
    if 'encode_queue' in df_pivot.columns and 'capture_start' in df_pivot.columns:
        df_pivot['latency_capture_ms'] = df_pivot['encode_queue'] - df_pivot['capture_start']
    
    # 2. Encode Latency Components
    
    # Time from Queue to Handle Input (Waiting for Event / Channel)
    if 'handle_input_start' in df_pivot.columns and 'encode_queue' in df_pivot.columns:
        df_pivot['latency_encode_wait_ms'] = df_pivot['handle_input_start'] - df_pivot['encode_queue']

    # Preprocessing Time (RGBA -> NV12)
    if 'preprocess_end' in df_pivot.columns and 'preprocess_start' in df_pivot.columns:
        df_pivot['latency_encode_preprocess_ms'] = df_pivot['preprocess_end'] - df_pivot['preprocess_start']
        
    # Buffer Creation Time
    if 'buffer_create_end' in df_pivot.columns and 'buffer_create_start' in df_pivot.columns:
        df_pivot['latency_encode_buffer_ms'] = df_pivot['buffer_create_end'] - df_pivot['buffer_create_start']

    # Pre-processing Total (Queue to ProcessInput)
    if 'process_input_start' in df_pivot.columns and 'encode_queue' in df_pivot.columns:
        df_pivot['latency_encode_pre_total_ms'] = df_pivot['process_input_start'] - df_pivot['encode_queue']
        
    # Submission time (ProcessInput)
    if 'process_input_end' in df_pivot.columns and 'process_input_start' in df_pivot.columns:
        df_pivot['latency_encode_submit_ms'] = df_pivot['process_input_end'] - df_pivot['process_input_start']
        
    # GPU Processing Time (approx, time between input end and output start)
    if 'process_output_start' in df_pivot.columns and 'process_input_end' in df_pivot.columns:
        df_pivot['latency_encode_gpu_ms'] = df_pivot['process_output_start'] - df_pivot['process_input_end']

    # Retrieval time (ProcessOutput)
    if 'process_output_end' in df_pivot.columns and 'process_output_start' in df_pivot.columns:
        df_pivot['latency_encode_retrieve_ms'] = df_pivot['process_output_end'] - df_pivot['process_output_start']
    
    # Post-processing / Post-encode to Send
    if 'send_start' in df_pivot.columns and 'process_output_end' in df_pivot.columns:
        df_pivot['latency_encode_post_ms'] = df_pivot['send_start'] - df_pivot['process_output_end']

    # Total Encode Latency
    if 'send_start' in df_pivot.columns and 'encode_queue' in df_pivot.columns:
        df_pivot['latency_encode_total_ms'] = df_pivot['send_start'] - df_pivot['encode_queue']

    # Total Frame Latency
    if 'send_start' in df_pivot.columns and 'capture_start' in df_pivot.columns:
        df_pivot['latency_total_ms'] = df_pivot['send_start'] - df_pivot['capture_start']

    # Select latency columns
    stats_cols = [c for c in df_pivot.columns if c.startswith('latency_')]
    valid_frames = df_pivot

    return {
        "frame_count": len(valid_frames),
        "stats_cols": stats_cols,
        "valid_frames": valid_frames,
    }


def format_results(result, output_format: str) -> str:
    """Format analysis results as text, json, or markdown."""
    stats_cols = result["stats_cols"]
    valid_frames = result["valid_frames"]
    frame_count = result["frame_count"]

    if not stats_cols:
        if output_format == "json":
            return json.dumps({"frame_count": frame_count, "metrics": {}})
        return f"Trace Analysis Summary (Frames found: {frame_count})\nNo latency metrics could be calculated."

    summary = valid_frames[stats_cols].describe(percentiles=[0.5, 0.9, 0.95, 0.99])

    if output_format == "json":
        metrics = {}
        for col in stats_cols:
            s = valid_frames[col].dropna()
            metrics[col] = {
                "count": int(s.count()),
                "mean": round(float(s.mean()), 4),
                "std": round(float(s.std()), 4) if s.std() == s.std() else 0,
                "min": round(float(s.min()), 4),
                "p50": round(float(s.quantile(0.5)), 4),
                "p90": round(float(s.quantile(0.9)), 4),
                "p95": round(float(s.quantile(0.95)), 4),
                "p99": round(float(s.quantile(0.99)), 4),
                "max": round(float(s.max()), 4),
            }
        return json.dumps({"frame_count": frame_count, "metrics": metrics}, indent=2)

    if output_format == "markdown":
        lines = [
            f"# RemoteRG Trace Analysis",
            f"**Frames analyzed:** {frame_count}",
            "",
            "## Latency Metrics (ms)",
            "",
            "| Metric | Count | Mean | Std | Min | P50 | P90 | P95 | P99 | Max |",
            "|--------|-------|------|-----|-----|-----|-----|-----|-----|-----|",
        ]
        for col in stats_cols:
            s = valid_frames[col].dropna()
            row = (
                col,
                int(s.count()),
                round(float(s.mean()), 2),
                round(float(s.std()), 2) if s.std() == s.std() else 0,
                round(float(s.min()), 2),
                round(float(s.quantile(0.5)), 2),
                round(float(s.quantile(0.9)), 2),
                round(float(s.quantile(0.95)), 2),
                round(float(s.quantile(0.99)), 2),
                round(float(s.max()), 2),
            )
            lines.append(f"| {row[0]} | {row[1]} | {row[2]} | {row[3]} | {row[4]} | {row[5]} | {row[6]} | {row[7]} | {row[8]} | {row[9]} |")
        return "\n".join(lines)

    # text (default)
    lines = [
        "\n" + "=" * 50,
        f"Trace Analysis Summary (Frames found: {frame_count})",
        "=" * 50,
        "",
        str(summary.transpose().round(2)),
        "",
        "=" * 50,
        "Detailed Latency Breakdown (Avg ms)",
        "=" * 50,
        str(valid_frames[stats_cols].mean().round(4)),
    ]
    return "\n".join(lines)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze RemoteRG trace-timestamp.json")
    parser.add_argument("file", help="Path to trace-timestamp.json file")
    parser.add_argument(
        "--format",
        choices=["text", "json", "markdown"],
        default="text",
        help="Output format (default: text)",
    )
    args = parser.parse_args()

    result = analyze_trace(args.file)
    if result is None:
        sys.exit(1)
    print(format_results(result, args.format))
