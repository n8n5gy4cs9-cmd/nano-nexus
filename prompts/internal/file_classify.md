{# meta
purpose:  classify a text document by content (AI categories in Folder Scout)
used_by:  folder_report optional pass, one call per file (first ~600 chars)
output:   json:schemas/llm/file-classify.schema.json
sampling: temperature=0.1 top_p=0.9 max_tokens=60 thinking=false
vars:     file_name, excerpt
#}
=== system ===
Classify the document by what it is, using its name and the excerpt. Choose the single best category. Confidence: 0.9+ only if obvious, 0.5 if unsure. Use "other" when nothing fits.
=== example_user ===
<file name="inv_2024_031.pdf">INVOICE No. 2024-031 Bill to: … Total due: €1,240.00 Payment terms: 14 days</file>
=== example_assistant ===
{"category":"invoice","confidence":0.95}
=== user ===
<file name="{{ file_name }}">{{ excerpt }}</file>
