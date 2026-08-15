use std::fmt::Write;

pub const WORKSPACE_SIZES: [usize; 3] = [100, 1_000, 10_000];

const REQUESTS_PER_FOLDER: usize = 100;

/// Builds a deterministic, bundled OpenCollection workspace representative of
/// an API collection with folders, headers, query parameters, and JSON bodies.
pub fn bundled_workspace(request_count: usize) -> String {
    assert!(request_count > 0);

    let mut source = String::with_capacity(request_count * 600);
    source.push_str(
        "opencollection: 1.0.0\ninfo:\n  name: Performance fixture\nbundled: true\nitems:\n",
    );

    for folder_start in (0..request_count).step_by(REQUESTS_PER_FOLDER) {
        let folder_index = folder_start / REQUESTS_PER_FOLDER;
        writeln!(
            source,
            "  - info:\n      name: Resource {folder_index:04}\n      type: folder\n      seq: {folder_index}\n    items:"
        )
        .expect("writing to a String cannot fail");

        let folder_end = (folder_start + REQUESTS_PER_FOLDER).min(request_count);
        for request_index in folder_start..folder_end {
            write!(
                source,
                concat!(
                    "      - info:\n",
                    "          name: Request {request_index:05}\n",
                    "          type: http\n",
                    "          seq: {request_index}\n",
                    "        http:\n",
                    "          method: POST\n",
                    "          url: https://api.example.com/resources/{request_index}\n",
                    "          headers:\n",
                    "            - name: Accept\n",
                    "              value: application/json\n",
                    "            - name: X-Request-Id\n",
                    "              value: request-{request_index:05}\n",
                    "          params:\n",
                    "            - name: page\n",
                    "              value: \"1\"\n",
                    "              type: query\n",
                    "            - name: limit\n",
                    "              value: \"50\"\n",
                    "              type: query\n",
                    "          body:\n",
                    "            type: json\n",
                    "            data: '{{\"request\":{request_index},\"active\":true}}'\n",
                ),
                request_index = request_index,
            )
            .expect("writing to a String cannot fail");
        }
    }

    source
}
