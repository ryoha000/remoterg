import json
import pandas as pd
import argparse
import sys
from pathlib import Path

def analyze_trace(file_path):
    print(f"Loading trace file: {file_path}")
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except FileNotFoundError:
        print(f"Error: File not found: {file_path}")
        return
    except json.JSONDecodeError:
        print(f"Error: Invalid JSON file: {file_path}")
        return

    # Extract events
    if isinstance(data, list):
        events = data
    elif isinstance(data, dict):
        events = data.get('traceEvents', [])
    else:
        print("Error: Unknown trace file format (not a list or dict)")
        return

    if not events:
        print("Error: No traceEvents found")
        return

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
        print("No relevant frame events found in trace.")
        return

    df = pd.DataFrame(frame_events)
    
    # Pivot to have one row per frame_id (using 'first' to handle duplicates if any)
    df_pivot = df.pivot_table(index='frame_id', columns='event', values='ts', aggfunc='first')
    
    # Debug: Print available columns
    print(f"DEBUG: Available event columns: {df_pivot.columns.tolist()}")

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

    print("\n" + "="*50)
    print(f"Trace Analysis Summary (Frames found: {len(valid_frames)})")
    print("="*50)
    
    if stats_cols:
        summary = valid_frames[stats_cols].describe(percentiles=[0.5, 0.9, 0.95, 0.99])
        print(summary.transpose().round(2))
        
        print("\n" + "="*50)
        print("Detailed Latency Breakdown (Avg ms)")
        print("="*50)
        print(valid_frames[stats_cols].mean().round(4))
    else:
        print("No latency metrics could be calculated.")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze RemoteRG trace-timestamp.json")
    parser.add_argument("file", help="Path to trace-timestamp.json file")
    args = parser.parse_args()
    
    analyze_trace(args.file)
