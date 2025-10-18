#!/usr/bin/env python3
"""
Comprehensive Arc<Mutex<T>> audit tool.
Analyzes all Arc<Mutex> uses to determine necessity and alternatives.
"""

import re
import os
from pathlib import Path
from collections import defaultdict
import json

def find_rust_files(root_dir):
    """Find all .rs files excluding target directory."""
    files = []
    for rs_file in Path(root_dir).rglob("*.rs"):
        # Exclude target directory
        if '/target/' not in str(rs_file):
            files.append(rs_file)
    return files

def is_test_file(file_path):
    """Determine if a file is test code."""
    path_str = str(file_path)
    return (
        '/tests/' in path_str or
        '/test/' in path_str or
        path_str.endswith('_test.rs') or
        path_str.endswith('_tests.rs') or
        'test_' in path_str or
        'tests.rs' in path_str
    )

def extract_arc_mutex_usage(file_path):
    """Extract all Arc<Mutex<...>> usage from a file."""
    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()
        lines = content.split('\n')

    # Pattern to find Arc<Mutex<...>>
    pattern = r'Arc<Mutex<([^>]+(?:<[^>]+>)?)>>'

    matches = []
    for line_num, line in enumerate(lines, 1):
        for match in re.finditer(pattern, line):
            inner_type = match.group(1)
            context_start = max(0, line_num - 3)
            context_end = min(len(lines), line_num + 3)
            context = '\n'.join(lines[context_start:context_end])

            matches.append({
                'file': str(file_path),
                'line': line_num,
                'inner_type': inner_type,
                'full_match': match.group(0),
                'line_content': line.strip(),
                'context': context
            })

    return matches

def analyze_usage_pattern(usage, file_content):
    """Analyze if Arc<Mutex> is necessary."""
    issues = []
    recommendations = []

    inner_type = usage['inner_type']
    line_content = usage['line_content']
    context = usage['context']

    # Check if it's a simple type that might not need locking
    simple_types = ['bool', 'i32', 'i64', 'u32', 'u64', 'f32', 'f64', 'usize', 'isize']
    if any(inner_type.startswith(t) for t in simple_types):
        issues.append("Simple atomic type - consider using atomic primitives")
        recommendations.append(f"Use Atomic{inner_type.capitalize()} instead")

    # Check if it's used with .clone() frequently
    clone_count = file_content.count(f".clone()")
    if clone_count > 5:
        issues.append(f"File has {clone_count} .clone() calls - possible excessive sharing")

    # Check for read-heavy patterns
    lock_pattern = r'\.lock\(\)\.(?:unwrap\(\)|expect)'
    read_only_patterns = [
        r'\.lock\(\).*\.get\(',
        r'\.lock\(\).*\.contains',
        r'\.lock\(\).*\.iter\(',
        r'let .* = .*\.lock\(\)',
    ]

    locks_in_context = len(re.findall(lock_pattern, context))
    reads_in_context = sum(len(re.findall(p, context)) for p in read_only_patterns)

    if reads_in_context > locks_in_context * 0.7:
        issues.append("Appears to be read-heavy")
        recommendations.append("Consider Arc<RwLock<T>> for better read concurrency")

    # Check if it's actually shared across threads
    thread_keywords = ['spawn', 'thread::', 'tokio::spawn', 'async', 'send', 'sync']
    has_threading = any(keyword in file_content.lower() for keyword in thread_keywords)

    if not has_threading:
        issues.append("No obvious threading/async usage in file")
        recommendations.append("Might not need Arc - consider Rc<RefCell<T>> or plain ownership")

    # Check for channel alternatives
    if 'sender' in inner_type.lower() or 'receiver' in inner_type.lower() or 'channel' in file_content.lower():
        recommendations.append("Consider using channels for producer/consumer pattern")

    # Defensive programming check
    if '.clone()' in line_content and 'Arc::new(Mutex::new' in line_content:
        issues.append("Created and immediately cloned - might be defensive programming")

    return {
        'issues': issues,
        'recommendations': recommendations,
        'severity': 'HIGH' if len(issues) >= 3 else 'MEDIUM' if len(issues) >= 1 else 'LOW'
    }

def main():
    root_dir = "/Users/nickpaterno/work/turbomcp"

    print("=" * 80)
    print("Arc<Mutex<T>> AUDIT - COMPREHENSIVE ANALYSIS")
    print("=" * 80)
    print()

    all_usages = []
    test_usages = []
    prod_usages = []

    for rs_file in find_rust_files(root_dir):
        usages = extract_arc_mutex_usage(rs_file)
        if usages:
            # Read full file content for analysis
            with open(rs_file, 'r', encoding='utf-8') as f:
                file_content = f.read()

            for usage in usages:
                analysis = analyze_usage_pattern(usage, file_content)
                usage['analysis'] = analysis

                if is_test_file(rs_file):
                    test_usages.append(usage)
                else:
                    prod_usages.append(usage)

                all_usages.append(usage)

    print(f"Total Arc<Mutex> occurrences: {len(all_usages)}")
    print(f"  Test code: {len(test_usages)}")
    print(f"  Production code: {len(prod_usages)}")
    print()

    # Group by file
    by_file = defaultdict(list)
    for usage in all_usages:
        by_file[usage['file']].append(usage)

    print("Top files by Arc<Mutex> count:")
    sorted_files = sorted(by_file.items(), key=lambda x: len(x[1]), reverse=True)
    for file, usages in sorted_files[:10]:
        is_test = is_test_file(file)
        tag = "[TEST]" if is_test else "[PROD]"
        print(f"  {tag} {len(usages):3d} - {file.split('turbomcp/')[-1]}")
    print()

    # Analyze production code by severity
    print("=" * 80)
    print("PRODUCTION CODE ANALYSIS")
    print("=" * 80)
    print()

    by_severity = defaultdict(list)
    for usage in prod_usages:
        severity = usage['analysis']['severity']
        by_severity[severity].append(usage)

    for severity in ['HIGH', 'MEDIUM', 'LOW']:
        count = len(by_severity[severity])
        print(f"\n{severity} PRIORITY: {count} occurrences")
        print("-" * 80)

        for usage in by_severity[severity][:5]:  # Show top 5 per severity
            print(f"\n  File: {usage['file'].split('turbomcp/')[-1]}:{usage['line']}")
            print(f"  Type: Arc<Mutex<{usage['inner_type']}>>")
            print(f"  Line: {usage['line_content'][:100]}")

            if usage['analysis']['issues']:
                print(f"  Issues:")
                for issue in usage['analysis']['issues']:
                    print(f"    - {issue}")

            if usage['analysis']['recommendations']:
                print(f"  Recommendations:")
                for rec in usage['analysis']['recommendations']:
                    print(f"    - {rec}")

        if count > 5:
            print(f"\n  ... and {count - 5} more")

    # Detailed breakdown by inner type
    print()
    print("=" * 80)
    print("BREAKDOWN BY INNER TYPE")
    print("=" * 80)
    print()

    by_type = defaultdict(list)
    for usage in prod_usages:
        by_type[usage['inner_type']].append(usage)

    sorted_types = sorted(by_type.items(), key=lambda x: len(x[1]), reverse=True)
    for inner_type, usages in sorted_types[:15]:
        print(f"  {len(usages):3d} - Arc<Mutex<{inner_type}>>")

    # Save detailed report
    report = {
        'summary': {
            'total': len(all_usages),
            'test': len(test_usages),
            'production': len(prod_usages),
            'by_severity': {k: len(v) for k, v in by_severity.items()},
            'unique_types': len(by_type)
        },
        'production_usages': [
            {
                'file': u['file'],
                'line': u['line'],
                'type': u['inner_type'],
                'severity': u['analysis']['severity'],
                'issues': u['analysis']['issues'],
                'recommendations': u['analysis']['recommendations']
            }
            for u in prod_usages
        ],
        'top_files': [
            {
                'file': f,
                'count': len(u),
                'is_test': is_test_file(f)
            }
            for f, u in sorted_files[:20]
        ]
    }

    with open('/Users/nickpaterno/work/turbomcp/arc_mutex_audit_report.json', 'w') as f:
        json.dump(report, f, indent=2)

    print()
    print("=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print()
    print(f"Total Arc<Mutex> occurrences: {len(all_usages)}")
    print(f"Production code occurrences: {len(prod_usages)}")
    print(f"  HIGH priority: {len(by_severity['HIGH'])}")
    print(f"  MEDIUM priority: {len(by_severity['MEDIUM'])}")
    print(f"  LOW priority: {len(by_severity['LOW'])}")
    print()
    print("Detailed report saved to: arc_mutex_audit_report.json")
    print("=" * 80)

if __name__ == '__main__':
    main()
