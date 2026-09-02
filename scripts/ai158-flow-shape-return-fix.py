from pathlib import Path

path = Path('crates/aikit-core/src/flow.rs')
source = path.read_text()

old = '''                vec![crate::ReadingReturnPath {
                    from_basis: flow_ref.clone(),
                    through: vec![relational.anchor_ref.clone()],
                    to_whole: generated_ref.clone(),
                }],
'''
new = '''                vec![
                    crate::ReadingReturnPath {
                        from_basis: flow_ref.clone(),
                        through: vec![relational.anchor_ref.clone()],
                        to_whole: generated_ref.clone(),
                    },
                    crate::ReadingReturnPath {
                        from_basis: site.conjugate_ref.clone(),
                        through: vec![relational.anchor_ref.clone()],
                        to_whole: generated_ref.clone(),
                    },
                ],
'''
if source.count(old) != 1:
    raise SystemExit(f'expected one relational Return-path anchor, found {source.count(old)}')
path.write_text(source.replace(old, new, 1))
