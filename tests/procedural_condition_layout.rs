mod support;

#[test]
fn exists_condition_uses_canonical_query_layout() {
    support::assert_sql_layout_only(
        "DO $$ BEGIN IF EXISTS (SELECT sample_id FROM demo.measurements WHERE sample_label = 'calibration' AND sampling_interval_seconds > 123 AND quality_score > 9) THEN PERFORM 1; END IF; END; $$;",
        "DO $$\nBEGIN\n    IF EXISTS (\n        SELECT sample_id\n        FROM demo.measurements\n        WHERE sample_label = 'calibration' AND sampling_interval_seconds > 123 AND quality_score > 9\n    ) THEN\n        PERFORM 1;\n    END IF;\nEND;\n$$;",
    );
}

#[test]
fn unreviewed_condition_expression_preserves_the_routine() {
    support::assert_unsupported(
        "do $$ begin if json_value(payload, '$.enabled') is not null then perform 1; end if; end $$;",
    );
}

#[test]
fn line_comment_before_then_stays_attached_to_the_condition() {
    support::assert_sql_layout_only(
        "DO $$ BEGIN IF TRUE -- preserve this condition comment\nTHEN PERFORM 1; END IF; END; $$;",
        "DO $$\nBEGIN\n    IF TRUE -- preserve this condition comment\n    THEN\n        PERFORM 1;\n    END IF;\nEND;\n$$;",
    );
}

#[test]
fn compact_if_elsif_and_while_keep_their_header_boundaries() {
    support::assert_sql_layout_only(
        "do $$ declare mode boolean := true; begin if mode then perform 1; elsif not mode then perform 2; end if; while mode loop exit; end loop; end $$;",
        "DO $$\nDECLARE\n    mode boolean := TRUE;\nBEGIN\n    IF mode THEN\n        PERFORM 1;\n    ELSIF NOT mode THEN\n        PERFORM 2;\n    END IF;\n    WHILE mode LOOP\n        EXIT;\n    END LOOP;\nEND\n$$;",
    );
}

#[test]
fn nested_condition_queries_preserve_comments_literals_and_crlf() {
    let condition = "EXISTS (\nSELECT sample_id\nFROM demo.measurements\n-- THEN and LOOP are comment text\nWHERE sample_label = 'THEN LOOP'\n\nAND quality_score > 9\n)";
    for (prefix, suffix, trailer) in [
        ("IF", "THEN", "END IF;"),
        ("WHILE", "LOOP", "END LOOP;"),
        ("IF FALSE THEN PERFORM 0; ELSIF", "THEN", "END IF;"),
        ("IF NOT", "THEN", "END IF;"),
    ] {
        let source = format!(
            "DO $body$ BEGIN {prefix} {condition} {suffix} PERFORM 1; {trailer} END; $body$;"
        );
        for source in [source.clone(), source.replace('\n', "\r\n")] {
            let options = semblock::FormatOptions {
                unsupported_policy: semblock::UnsupportedPolicy::Error,
                ..Default::default()
            };
            let formatted = semblock::format_sql(&source, &options).unwrap();
            assert!(
                formatted
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.fix_available),
                "{:?}",
                formatted.diagnostics
            );
            assert!(
                formatted
                    .output
                    .contains("-- THEN and LOOP are comment text")
            );
            assert!(formatted.output.contains("'THEN LOOP'"));
            assert!(formatted.output.contains("\n\n") || formatted.output.contains("\r\n\r\n"));
            assert_eq!(
                semblock::format_sql(&formatted.output, &options)
                    .unwrap()
                    .output,
                formatted.output
            );
            assert!(semblock::check_sql(&formatted.output, &options).compliant);
            if source.contains("\r\n") {
                assert!(!formatted.output.replace("\r\n", "").contains('\n'));
            }
        }
    }
}

#[test]
fn long_boolean_condition_wraps_with_its_procedural_indent() {
    let condition = (0..12)
        .map(|index| format!("quality_{index} > 0"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let source =
        format!("DO $$ BEGIN IF TRUE THEN IF {condition} THEN PERFORM 1; END IF; END IF; END; $$;");
    let options = semblock::FormatOptions {
        soft_line_width: 70,
        hard_line_width: 90,
        ..Default::default()
    };
    let formatted = semblock::format_sql(&source, &options).unwrap();
    assert!(
        formatted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        formatted.diagnostics
    );
    assert!(
        formatted
            .output
            .lines()
            .all(|line| line.chars().count() <= 90)
    );
    assert!(formatted.output.contains("\n        IF quality_0 > 0"));
    assert_eq!(
        semblock::format_sql(&formatted.output, &options)
            .unwrap()
            .output,
        formatted.output
    );
}
