from perfetto.trace_processor import TraceProcessor
import sys
import pandas as pd


def main():
    trace_path = sys.argv[1] if len(sys.argv) > 1 else '../android/cpu-perfetto-20260222T113449.trace'
    print(f"Loading trace: {trace_path}...")
    try:
        tp = TraceProcessor(trace=trace_path)
    except Exception as e:
        print(f"Failed to load trace: {e}")
        return
    
    pd.set_option('display.max_columns', None)
    pd.set_option('display.width', 1000)
    pd.set_option('display.max_colwidth', 80)

    # --- 1. プロセス一覧 ---
    print("\n" + "="*80)
    print("1. プロセス一覧")
    print("="*80)
    try:
        procs = tp.query('''
            SELECT upid, name, pid FROM process WHERE name LIKE '%remoterg%'
        ''').as_pandas_dataframe()
        print(procs)
    except Exception as e:
        print("Error:", e)

    # --- 2. スレッド別CPU時間 ---
    print("\n" + "="*80)
    print("2. スレッド別CPU時間 (remoterg プロセスのみ)")
    print("="*80)
    try:
        threads = tp.query('''
            SELECT t.name as thread_name, SUM(s.dur) / 1e6 AS cpu_time_ms, COUNT(*) as slice_count
            FROM thread t
            JOIN process p ON t.upid = p.upid
            JOIN thread_track tt ON tt.utid = t.utid
            JOIN slice s ON s.track_id = tt.id
            WHERE p.name LIKE '%remoterg%'
            GROUP BY t.name
            ORDER BY cpu_time_ms DESC
            LIMIT 15
        ''').as_pandas_dataframe()
        print(threads)
    except Exception as e:
        print("Error:", e)
    
    # --- 3. 集約スライス (1ms 以上) ---
    print("\n" + "="*80)
    print("3. 集約スライス (1ms 以上, remoterg プロセス)")
    print("="*80)
    try:
        long_main = tp.query('''
            SELECT t.name as thread_name, s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms,
                   ROUND(AVG(s.dur)/1e6, 2) as avg_ms,
                   ROUND(MAX(s.dur)/1e6, 2) as max_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND s.dur > 1000000
            GROUP BY t.name, s.name
            ORDER BY total_ms DESC
            LIMIT 25
        ''').as_pandas_dataframe()
        print(long_main)
    except Exception as e:
        print("Error:", e)

    # --- 4. メインスレッドの重いスライス (個別) ---
    print("\n" + "="*80)
    print("4. メインスレッド・RenderThread の重い個別スライス (Top 20)")
    print("="*80)
    try:
        longest_individual = tp.query('''
            SELECT t.name as thread_name, s.name as slice_name,
                   ROUND(s.dur/1e6, 2) as dur_ms,
                   s.ts
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND t.name IN ('.ryoha.remoterg', 'RenderThread')
              AND s.name NOT LIKE '%sleep%'
              AND s.name NOT LIKE '%waiting%'
              AND s.dur > 100000
            ORDER BY dur_ms DESC
            LIMIT 20
        ''').as_pandas_dataframe()
        pd.set_option('display.max_colwidth', None)
        print(longest_individual)
    except Exception as e:
        print("Error:", e)

    # --- 5. Compose リコンポジション分析 ---
    print("\n" + "="*80)
    print("5. Compose リコンポジション分析")
    print("="*80)
    try:
        recompose = tp.query('''
            SELECT s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms,
                   ROUND(AVG(s.dur)/1e6, 2) as avg_ms,
                   ROUND(MAX(s.dur)/1e6, 2) as max_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND (s.name LIKE '%recompos%'
                OR s.name LIKE '%Compose%'
                OR s.name LIKE '%Measure%'
                OR s.name LIKE '%Layout%'
                OR s.name LIKE '%Draw%'
                OR s.name LIKE 'Choreographer%')
            GROUP BY s.name
            ORDER BY total_ms DESC
            LIMIT 20
        ''').as_pandas_dataframe()
        print(recompose)
    except Exception as e:
        print("Error:", e)

    # --- 6. Coil / 画像デコード関連 ---
    print("\n" + "="*80)
    print("6. 画像デコード・Coil 関連スライス")
    print("="*80)
    try:
        image = tp.query('''
            SELECT t.name as thread_name, s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms,
                   ROUND(AVG(s.dur)/1e6, 2) as avg_ms,
                   ROUND(MAX(s.dur)/1e6, 2) as max_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND (s.name LIKE '%decode%'
                OR s.name LIKE '%Bitmap%'
                OR s.name LIKE '%image%'
                OR s.name LIKE '%Coil%'
                OR s.name LIKE '%texture%'
                OR s.name LIKE '%upload%'
                OR s.name LIKE '%OpenGL%'
                OR s.name LIKE '%GPU%')
            GROUP BY t.name, s.name
            ORDER BY total_ms DESC
            LIMIT 20
        ''').as_pandas_dataframe()
        print(image)
    except Exception as e:
        print("Error:", e)

    # --- 7. フレームタイミング分析 ---
    print("\n" + "="*80)
    print("7. フレームタイミング分析")
    print("="*80)
    try:
        # actual_frame_timeline_slice を試行
        frames = tp.query('''
            SELECT
                COUNT(*) as total_frames,
                ROUND(AVG(dur)/1e6, 2) as avg_frame_dur_ms,
                ROUND(MAX(dur)/1e6, 2) as max_frame_dur_ms,
                ROUND(MIN(dur)/1e6, 2) as min_frame_dur_ms,
                SUM(CASE WHEN dur > 16666666 THEN 1 ELSE 0 END) as jank_frames_16ms,
                SUM(CASE WHEN dur > 33333333 THEN 1 ELSE 0 END) as jank_frames_33ms,
                SUM(CASE WHEN dur > 50000000 THEN 1 ELSE 0 END) as jank_frames_50ms
            FROM actual_frame_timeline_slice
            JOIN process USING(upid)
            WHERE process.name LIKE '%remoterg%'
        ''').as_pandas_dataframe()
        print(frames)
    except Exception as e:
        print(f"actual_frame_timeline_slice not available: {e}")
        # フォールバック: Choreographer#doFrame で推定
        try:
            frames_fb = tp.query('''
                SELECT
                    COUNT(*) as frame_count,
                    ROUND(AVG(s.dur)/1e6, 2) as avg_dur_ms,
                    ROUND(MAX(s.dur)/1e6, 2) as max_dur_ms,
                    SUM(CASE WHEN s.dur > 16666666 THEN 1 ELSE 0 END) as jank_16ms,
                    SUM(CASE WHEN s.dur > 33333333 THEN 1 ELSE 0 END) as jank_33ms
                FROM slice s
                JOIN thread_track tt ON s.track_id = tt.id
                JOIN thread t ON t.utid = tt.utid
                JOIN process p ON t.upid = p.upid
                WHERE p.name LIKE '%remoterg%'
                  AND s.name LIKE 'Choreographer%'
            ''').as_pandas_dataframe()
            print("(Choreographer#doFrame ベース)")
            print(frames_fb)
        except Exception as e2:
            print("Error:", e2)

    # --- 8. Jank フレーム詳細 ---
    print("\n" + "="*80)
    print("8. Jank フレーム (16ms超) の詳細 (Top 15)")
    print("="*80)
    try:
        jank = tp.query('''
            SELECT
                ts,
                ROUND(dur/1e6, 2) as dur_ms,
                jank_type,
                jank_tag,
                on_time_finish
            FROM actual_frame_timeline_slice
            JOIN process USING(upid)
            WHERE process.name LIKE '%remoterg%'
              AND dur > 16666666
            ORDER BY dur DESC
            LIMIT 15
        ''').as_pandas_dataframe()
        print(jank)
    except Exception as e:
        print(f"actual_frame_timeline_slice not available: {e}")
        # フォールバック
        try:
            jank_fb = tp.query('''
                SELECT s.name as slice_name, s.ts,
                       ROUND(s.dur/1e6, 2) as dur_ms
                FROM slice s
                JOIN thread_track tt ON s.track_id = tt.id
                JOIN thread t ON t.utid = tt.utid
                JOIN process p ON t.upid = p.upid
                WHERE p.name LIKE '%remoterg%'
                  AND t.name = '.ryoha.remoterg'
                  AND s.name LIKE 'Choreographer%'
                  AND s.dur > 16666666
                ORDER BY s.dur DESC
                LIMIT 15
            ''').as_pandas_dataframe()
            print("(Choreographer#doFrame ベース)")
            print(jank_fb)
        except Exception as e2:
            print("Error:", e2)

    # --- 9. GC (ガベージコレクション) ---
    print("\n" + "="*80)
    print("9. GC (ガベージコレクション) イベント")
    print("="*80)
    try:
        gc = tp.query('''
            SELECT s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms,
                   ROUND(AVG(s.dur)/1e6, 2) as avg_ms,
                   ROUND(MAX(s.dur)/1e6, 2) as max_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND (s.name LIKE '%GC%' OR s.name LIKE '%garbage%' OR s.name LIKE '%HeapTaskDaemon%')
            GROUP BY s.name
            ORDER BY total_ms DESC
            LIMIT 10
        ''').as_pandas_dataframe()
        if len(gc) == 0:
            print("GC イベントなし (Perfetto設定でキャプチャされていない可能性)")
        else:
            print(gc)
    except Exception as e:
        print("Error:", e)

    # --- 10. Binder トランザクション ---
    print("\n" + "="*80)
    print("10. Binder トランザクション (メインスレッド)")
    print("="*80)
    try:
        binder = tp.query('''
            SELECT s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms,
                   ROUND(AVG(s.dur)/1e6, 2) as avg_ms,
                   ROUND(MAX(s.dur)/1e6, 2) as max_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND t.name = '.ryoha.remoterg'
              AND (s.name LIKE '%binder%' OR s.name LIKE '%Binder%')
            GROUP BY s.name
            ORDER BY total_ms DESC
            LIMIT 10
        ''').as_pandas_dataframe()
        if len(binder) == 0:
            print("メインスレッド上での Binder トランザクションなし")
        else:
            print(binder)
    except Exception as e:
        print("Error:", e)

    # --- 11. Navigation / Activity / Fragment ライフサイクル ---
    print("\n" + "="*80)
    print("11. Activity / Navigation ライフサイクル")
    print("="*80)
    try:
        lifecycle = tp.query('''
            SELECT s.name as slice_name, s.ts,
                   ROUND(s.dur/1e6, 2) as dur_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND (s.name LIKE '%Activity%'
                OR s.name LIKE '%Fragment%'
                OR s.name LIKE '%navigation%'
                OR s.name LIKE '%Navigation%'
                OR s.name LIKE '%inflate%'
                OR s.name LIKE '%onCreate%'
                OR s.name LIKE '%onResume%'
                OR s.name LIKE '%onPause%'
                OR s.name LIKE '%onStart%'
                OR s.name LIKE '%onStop%')
            ORDER BY s.ts ASC
            LIMIT 20
        ''').as_pandas_dataframe()
        print(lifecycle)
    except Exception as e:
        print("Error:", e)

    # --- 12. DefaultDispatcher / IO スレッド分析 ---
    print("\n" + "="*80)
    print("12. バックグラウンドスレッド (DefaultDispatch / IO) 分析")
    print("="*80)
    try:
        bg_threads = tp.query('''
            SELECT t.name as thread_name, s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms,
                   ROUND(AVG(s.dur)/1e6, 2) as avg_ms,
                   ROUND(MAX(s.dur)/1e6, 2) as max_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
              AND (t.name LIKE '%DefaultDispatch%' OR t.name LIKE '%IO%' OR t.name LIKE '%worker%')
              AND s.dur > 500000
            GROUP BY t.name, s.name
            ORDER BY total_ms DESC
            LIMIT 20
        ''').as_pandas_dataframe()
        print(bg_threads)
    except Exception as e:
        print("Error:", e)

    # --- 13. 全スライス名の一覧 (ユニーク、出現回数順) ---
    print("\n" + "="*80)
    print("13. 全スライス名の一覧 (remoterg プロセス, 出現回数順, Top 30)")
    print("="*80)
    try:
        all_slices = tp.query('''
            SELECT s.name as slice_name,
                   COUNT(*) as count,
                   ROUND(SUM(s.dur)/1e6, 2) as total_ms
            FROM slice s
            JOIN thread_track tt ON s.track_id = tt.id
            JOIN thread t ON t.utid = tt.utid
            JOIN process p ON t.upid = p.upid
            WHERE p.name LIKE '%remoterg%'
            GROUP BY s.name
            ORDER BY total_ms DESC
            LIMIT 30
        ''').as_pandas_dataframe()
        print(all_slices)
    except Exception as e:
        print("Error:", e)

    tp.close()
    print("\n分析完了。")


if __name__ == '__main__':
    main()
