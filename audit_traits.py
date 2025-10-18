#!/usr/bin/env python3
"""
Comprehensive trait audit tool.
Finds all traits and analyzes their method count, usage patterns, and necessity.
"""

import re
import os
from pathlib import Path
from collections import defaultdict
import json

def find_rust_files(root_dir):
    """Find all .rs files in the project."""
    return list(Path(root_dir).rglob("*.rs"))

def extract_traits(file_path):
    """Extract trait definitions from a Rust file."""
    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    traits = []

    # Pattern to match trait definitions
    # Handles: pub trait, pub(crate) trait, trait
    trait_pattern = re.compile(
        r'^(?P<indent>\s*)(?:pub(?:\([^)]*\))?\s+)?trait\s+(?P<name>\w+)(?:<[^>]*>)?\s*(?::\s*[^{]*)?{',
        re.MULTILINE
    )

    for match in trait_pattern.finditer(content):
        trait_name = match.group('name')
        start_pos = match.end()

        # Find the matching closing brace
        brace_count = 1
        pos = start_pos
        trait_body_start = start_pos

        while pos < len(content) and brace_count > 0:
            if content[pos] == '{':
                brace_count += 1
            elif content[pos] == '}':
                brace_count -= 1
            pos += 1

        trait_body = content[trait_body_start:pos-1]

        # Count methods in trait body (fn keyword)
        # Exclude comments
        lines = trait_body.split('\n')
        method_count = 0
        methods = []

        for line in lines:
            # Remove comments
            line = re.sub(r'//.*$', '', line)
            # Check for fn definitions (not in strings)
            fn_matches = re.findall(r'\bfn\s+(\w+)', line)
            method_count += len(fn_matches)
            methods.extend(fn_matches)

        # Check for associated types
        assoc_types = re.findall(r'type\s+(\w+)', trait_body)

        # Check for generics/lifetimes
        has_generics = '<' in match.group(0)

        # Get full trait signature for context
        signature_end = match.end()
        signature_start = match.start()
        signature = content[signature_start:signature_end].strip()

        traits.append({
            'name': trait_name,
            'file': str(file_path),
            'line': content[:match.start()].count('\n') + 1,
            'method_count': method_count,
            'methods': methods,
            'assoc_types': assoc_types,
            'has_generics': has_generics,
            'signature': signature,
            'body': trait_body.strip()
        })

    return traits

def check_trait_usage(trait_name, root_dir):
    """Check how a trait is used in the codebase."""
    usage_patterns = {
        'impl_count': 0,
        'dyn_usage': 0,
        'bound_usage': 0,
        'impl_files': [],
        'dyn_files': [],
        'bound_files': []
    }

    # Search for implementations
    impl_pattern = f"impl.*{trait_name}"
    dyn_pattern = f"dyn {trait_name}"
    bound_pattern = f": {trait_name}"

    for rs_file in find_rust_files(root_dir):
        try:
            with open(rs_file, 'r', encoding='utf-8') as f:
                content = f.read()

            if re.search(impl_pattern, content):
                usage_patterns['impl_count'] += len(re.findall(impl_pattern, content))
                usage_patterns['impl_files'].append(str(rs_file))

            if re.search(dyn_pattern, content):
                usage_patterns['dyn_usage'] += len(re.findall(dyn_pattern, content))
                usage_patterns['dyn_files'].append(str(rs_file))

            if re.search(bound_pattern, content):
                usage_patterns['bound_usage'] += len(re.findall(bound_pattern, content))
                usage_patterns['bound_files'].append(str(rs_file))
        except Exception as e:
            continue

    return usage_patterns

def analyze_trait_necessity(trait_info, usage):
    """Determine if a trait is necessary or could be simplified."""
    reasons_to_keep = []
    reasons_to_remove = []

    # Single method trait
    if trait_info['method_count'] == 1:
        reasons_to_remove.append("Single method trait")

    # Has associated types
    if trait_info['assoc_types']:
        reasons_to_keep.append(f"Has associated types: {', '.join(trait_info['assoc_types'])}")

    # Has generics
    if trait_info['has_generics']:
        reasons_to_keep.append("Uses generics/lifetimes")

    # Used in trait objects
    if usage['dyn_usage'] > 0:
        reasons_to_keep.append(f"Used in trait objects ({usage['dyn_usage']} times)")

    # Multiple implementations
    if usage['impl_count'] > 1:
        reasons_to_keep.append(f"Multiple implementations ({usage['impl_count']})")
    elif usage['impl_count'] == 0:
        reasons_to_remove.append("No implementations found")
    elif usage['impl_count'] == 1:
        reasons_to_remove.append("Only one implementation")

    # Used as bounds
    if usage['bound_usage'] > 2:
        reasons_to_keep.append(f"Used as trait bound ({usage['bound_usage']} times)")

    # Determine recommendation
    if not reasons_to_keep and reasons_to_remove:
        return "DEFINITELY_REMOVE", reasons_to_keep, reasons_to_remove
    elif len(reasons_to_keep) <= 1 and len(reasons_to_remove) >= 2:
        return "MAYBE_REMOVE", reasons_to_keep, reasons_to_remove
    else:
        return "KEEP_AS_IS", reasons_to_keep, reasons_to_remove

def main():
    root_dir = "/Users/nickpaterno/work/turbomcp"

    print("=" * 80)
    print("TRAIT AUDIT - COMPREHENSIVE ANALYSIS")
    print("=" * 80)
    print()

    # Collect all traits
    all_traits = []
    for rs_file in find_rust_files(root_dir):
        traits = extract_traits(rs_file)
        all_traits.extend(traits)

    print(f"Total traits found: {len(all_traits)}")
    print()

    # Analyze by method count
    by_method_count = defaultdict(list)
    for trait in all_traits:
        by_method_count[trait['method_count']].append(trait)

    print("Traits by method count:")
    for count in sorted(by_method_count.keys()):
        print(f"  {count} methods: {len(by_method_count[count])} traits")
    print()

    # Analyze single-method traits
    single_method_traits = by_method_count[1]
    print(f"=" * 80)
    print(f"SINGLE-METHOD TRAITS ANALYSIS ({len(single_method_traits)} found)")
    print(f"=" * 80)
    print()

    categorized = {
        'DEFINITELY_REMOVE': [],
        'MAYBE_REMOVE': [],
        'KEEP_AS_IS': []
    }

    for idx, trait in enumerate(single_method_traits, 1):
        print(f"[{idx}/{len(single_method_traits)}] Analyzing {trait['name']}...")
        usage = check_trait_usage(trait['name'], root_dir)
        category, keep_reasons, remove_reasons = analyze_trait_necessity(trait, usage)

        categorized[category].append({
            'trait': trait,
            'usage': usage,
            'keep_reasons': keep_reasons,
            'remove_reasons': remove_reasons
        })

    print()
    print("=" * 80)
    print("CATEGORIZATION RESULTS")
    print("=" * 80)
    print()

    for category in ['DEFINITELY_REMOVE', 'MAYBE_REMOVE', 'KEEP_AS_IS']:
        print(f"\n{category}: {len(categorized[category])} traits")
        print("-" * 80)

        for item in categorized[category]:
            trait = item['trait']
            usage = item['usage']
            print(f"\n  Trait: {trait['name']}")
            print(f"  File: {trait['file']}:{trait['line']}")
            print(f"  Method: {trait['methods'][0] if trait['methods'] else 'N/A'}")
            print(f"  Implementations: {usage['impl_count']}")
            print(f"  Dyn usage: {usage['dyn_usage']}")

            if item['keep_reasons']:
                print(f"  Keep reasons: {', '.join(item['keep_reasons'])}")
            if item['remove_reasons']:
                print(f"  Remove reasons: {', '.join(item['remove_reasons'])}")

    # Save detailed report
    report = {
        'summary': {
            'total_traits': len(all_traits),
            'single_method_traits': len(single_method_traits),
            'by_method_count': {k: len(v) for k, v in by_method_count.items()},
            'categorization': {k: len(v) for k, v in categorized.items()}
        },
        'details': categorized
    }

    with open('/Users/nickpaterno/work/turbomcp/trait_audit_report.json', 'w') as f:
        json.dump(report, f, indent=2, default=str)

    print()
    print("=" * 80)
    print("Detailed report saved to: trait_audit_report.json")
    print("=" * 80)

if __name__ == '__main__':
    main()
