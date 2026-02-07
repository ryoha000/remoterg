import json
import pandas as pd
import argparse
from pathlib import Path

def inspect_outliers(file_path, threshold_ms=50):
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

    frame_events = []
    
    # Process events
    for e in events:
        name = e.get('name')
        ts = e.get('ts', 0) / 1000.0 # Convert to ms
        ph = e.get('ph')
        
        args = e.get('args', {})
        if not args:
            continue
            
        frame_id = None
        if 'frame_id' in args:
            try:
                frame_id = int(args['frame_id'])
            except (ValueError, TypeError):
                pass
        
        # Capture critical timestamps
        if name == 'handle_need_input' and ph == 'B':
            if frame_id is not None:
                frame_events.append({'frame_id': frame_id, 'event': 'handle_input_start', 'ts': ts})
        
        elif name == 'queue_encode_job':
            # This is roughly "encode_queue"
            if frame_id is not None:
                frame_events.append({'frame_id': frame_id, 'event': 'encode_queue', 'ts': ts})

        elif name == 'process_output' and ph == 'E':
            if frame_id is not None:
                frame_events.append({'frame_id': frame_id, 'event': 'process_output_end', 'ts': ts})

        elif name == 'capture_start' or (name == 'frame_processing' and ph == 'B'):
             if frame_id is not None:
                 frame_events.append({'frame_id': frame_id, 'event': 'capture_start', 'ts': ts})

    if not frame_events:
        print("No relevant frame events found in trace.")
        return

    df = pd.DataFrame(frame_events)
    df_pivot = df.pivot_table(index='frame_id', columns='event', values='ts', aggfunc='first')
    
    # Calculate Wait Latency
    if 'handle_input_start' in df_pivot.columns and 'encode_queue' in df_pivot.columns:
        df_pivot['latency_encode_wait_ms'] = df_pivot['handle_input_start'] - df_pivot['encode_queue']
    
    # Filter Outliers
    outliers = df_pivot[df_pivot['latency_encode_wait_ms'] > threshold_ms].copy()
    
    if outliers.empty:
        print(f"No frames found with latency_encode_wait_ms > {threshold_ms}ms")
        return

    print(f"\nFound {len(outliers)} frames with latency_encode_wait_ms > {threshold_ms}ms:")
    
    # Add context: previous frame info
    # Sort by frame_id to find previous frame easily
    df_pivot_sorted = df_pivot.sort_index()
    
    for frame_id, row in outliers.iterrows():
        wait_ms = row['latency_encode_wait_ms']
        queue_ts = row['encode_queue']
        handle_ts = row['handle_input_start']
        capture_ts = row.get('capture_start', float('nan'))
        
        print(f"\n--- Frame {frame_id} (Wait: {wait_ms:.2f}ms) ---")
        print(f"  Capture Start:      {capture_ts:.2f}")
        print(f"  Queue Encode:       {queue_ts:.2f} (+{queue_ts - capture_ts:.2f}ms from capture)")
        print(f"  Handle Input Start: {handle_ts:.2f} (+{wait_ms:.2f}ms from queue)")
        
        # Check previous frame
        prev_frame_id = frame_id - 1
        if prev_frame_id in df_pivot_sorted.index:
            prev_row = df_pivot_sorted.loc[prev_frame_id]
            prev_output_end = prev_row.get('process_output_end', float('nan'))
            prev_handle_start = prev_row.get('handle_input_start', float('nan'))
            
            print(f"  Prev Frame ({prev_frame_id}) Finished Output: {prev_output_end:.2f}")
           
    # Detailed Dump for Top Outliers
    print("\n=== Detailed Events for Top 3 Outliers ===")
    
    # Get top 3 outliers
    top_outliers = outliers.sort_values('latency_encode_wait_ms', ascending=False).head(3)
    target_frames = top_outliers.index.tolist()
    
    # Also include previous frames for context (frame_id - 1)
    prev_frames = [f - 1 for f in target_frames]
    target_frames.extend(prev_frames)
    target_frames = list(set(target_frames)) # Remove duplicates    
    # Also report on wait_for_event spans that are long
    long_waits = []
    
    # Re-scan for all events
    target_events = []
    
    for e in events:
        name = e.get('name')
        ts = e.get('ts', 0) / 1000.0
        dur = e.get('dur', 0) / 1000.0 if 'dur' in e else 0
        args = e.get('args', {})
        
        # Check for long wait_for_event
        if name == 'wait_for_event' and dur > threshold_ms:
            long_waits.append((ts, dur, args))

        if not args:
            continue
        
        f_id = None
        if 'frame_id' in args:
            try:
                f_id = int(args['frame_id'])
            except:
                pass
        
        # If frame_id matches a target frame
        if f_id in target_frames:
            target_events.append(e)
            
        # Also include frame_drop events for targets
        if name == 'frame_drop' and f_id in target_frames:
             target_events.append(e)

    # Sort by timestamp
    target_events.sort(key=lambda x: x.get('ts', 0))
    
    print(f"Top Outlier Frames: {target_frames}")

    for e in target_events:
        name = e.get('name')
        ph = e.get('ph')
        ts = e.get('ts', 0) / 1000.0
        dur = e.get('dur', 0) / 1000.0 if 'dur' in e else 0
        args = e.get('args', {})
        dur_str = f" (dur={dur:.2f}ms)" if 'dur' in e else ""
        print(f"[{ts:.2f}] {name} ({ph}){dur_str} - {args}")

    print("\n=== Long wait_for_event Spans (> {:.0f}ms) ===".format(threshold_ms))
    for ts, dur, args in long_waits:
        print(f"[{ts:.2f}] wait_for_event (dur={dur:.2f}ms)")
        
    print("\n=== MMF Initialization Spans ===")
    init_spans = ['mf_encoder_init', 'd3d_create_resources', 'preproc_create', 'encoder_create', 'encoder_start_streaming']
    
    # Store start times for B events: {thread_id: {name: start_ts}}
    # Note: This is simplistic and assumes no recursion of same event name on same thread
    span_starts = {}
    
    # Re-scan to match B and E
    for e in events:
        name = e.get('name')
        if name in init_spans:
            ph = e.get('ph')
            ts = e.get('ts', 0) / 1000.0
            tid = e.get('tid')
            
            if tid not in span_starts:
                span_starts[tid] = {}
                
            if ph == 'B':
                span_starts[tid][name] = ts
            elif ph == 'E':
                if name in span_starts[tid]:
                    start_ts = span_starts[tid].pop(name)
                    duration = ts - start_ts
                    print(f"[{start_ts:.2f}] {name} (dur={duration:.2f}ms)")
                else:
                    print(f"[{ts:.2f}] {name} (E) - unmatched start")



if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Inspect outlier frames")
    parser.add_argument("file", help="Path to trace-timestamp.json file")
    parser.add_argument("--threshold", type=float, default=50.0, help="Latency threshold in ms")
    args = parser.parse_args()
    
    inspect_outliers(args.file, args.threshold)
