# Murphy mruby cop test prelude (Phase 8 Task 4).
#
# Implements describe_cop, expect_offense, and expect_correction helpers in mruby,
# which are resolved via FFI bridge Murphy.__run_cop_on_source.

class Murphy
  class TestRunner
    attr_reader :cop_name, :failures

    def initialize(cop_name)
      @cop_name = cop_name
      @failures = []
      @last_clean_source = nil
      @last_actual_offenses = nil
    end

    def it(description, &block)
      begin
        instance_eval(&block)
      rescue => e
        @failures << "#{description}: #{e.message}\n#{e.backtrace.join("\n")}"
      end
    end

    def expect_offense(annotated_source)
      clean_source, expected_offenses = parse_annotations(annotated_source)
      @last_clean_source = clean_source

      # Murphy.__run_cop_on_source crosses FFI boundary to run isolated mruby cop
      blob = Murphy.__run_cop_on_source(@cop_name, clean_source)
      actual_offenses = parse_offenses_blob(blob)
      @last_actual_offenses = actual_offenses

      verify_offenses!(clean_source, expected_offenses, actual_offenses)
    end

    def expect_correction(expected_source)
      raise "expect_correction must be called after expect_offense" unless @last_clean_source

      corrected = @last_clean_source
      blobs = @last_actual_offenses.map { |o| o["autocorrect_blob"] }.compact.reject(&:empty?)
      blobs.each do |blob|
        corrected = apply_edits_bytes(corrected, blob)
      end

      if corrected != expected_source
        raise "Expected correction:\n#{expected_source}\n\nBut got:\n#{corrected}"
      end
    end

    private

    def sort_offenses!(arr)
      n = arr.size
      loop do
        swapped = false
        (n-1).times do |i|
          a = arr[i]
          b = arr[i+1]

          a_start = a[:start_offset] || a["start_offset"] || 0
          a_end = a[:end_offset] || a["end_offset"] || 0
          b_start = b[:start_offset] || b["start_offset"] || 0
          b_end = b[:end_offset] || b["end_offset"] || 0

          if (a_start > b_start) || (a_start == b_start && a_end > b_end)
            arr[i], arr[i+1] = arr[i+1], arr[i]
            swapped = true
          end
        end
        break unless swapped
      end
      arr
    end

    def parse_offenses_blob(blob)
      offenses = []
      pos = 0
      while pos < blob.bytesize
        rest = blob[pos..-1]

        sp1 = rest.index(" ")
        break unless sp1
        start_offset = rest[0...sp1].to_i

        rest2 = rest[sp1+1..-1]
        sp2 = rest2.index(" ")
        break unless sp2
        end_offset = rest2[0...sp2].to_i

        rest3 = rest2[sp2+1..-1]
        sp3 = rest3.index(" ")
        break unless sp3
        sev_len = rest3[0...sp3].to_i

        rest4 = rest3[sp3+1..-1]
        sp4 = rest4.index(" ")
        break unless sp4
        msg_len = rest4[0...sp4].to_i

        rest5 = rest4[sp4+1..-1]
        sp5 = rest5.index(" ")
        break unless sp5
        ac_len = rest5[0...sp5].to_i

        header_len = sp1 + 1 + sp2 + 1 + sp3 + 1 + sp4 + 1 + sp5 + 1
        pos += header_len

        severity = blob[pos, sev_len]
        pos += sev_len

        message = blob[pos, msg_len]
        pos += msg_len

        autocorrect_blob = blob[pos, ac_len]
        pos += ac_len

        offenses << {
          "start_offset" => start_offset,
          "end_offset" => end_offset,
          "severity" => severity,
          "message" => message,
          "autocorrect_blob" => autocorrect_blob
        }
      end
      offenses
    end

    def parse_annotations(annotated_source)
      lines = annotated_source.split("\n", -1)
      clean_lines = []
      expected = []

      current_byte_offset = 0

      i = 0
      while i < lines.size
        line = lines[i]
        stripped = line.lstrip
        if !stripped.empty? && stripped.start_with?("^") && clean_lines.any?
          spaces_count = line.size - stripped.size
          spaces = line[0...spaces_count]

          carats_count = 0
          while carats_count < stripped.size && stripped[carats_count] == "^"
            carats_count += 1
          end
          carats = stripped[0...carats_count]
          message = stripped[carats_count..-1].strip

          prev_line = clean_lines.last
          prev_line_offset = current_byte_offset - prev_line.bytesize - 1 # exclude \n

          start_offset = prev_line_offset + spaces.bytesize
          end_offset = start_offset + carats.bytesize

          expected << {
            start_offset: start_offset,
            end_offset: end_offset,
            message: message
          }
        else
          clean_lines << line
          current_byte_offset += line.bytesize + 1 # include \n
        end
        i += 1
      end

      [clean_lines.join("\n"), expected]
    end

    def verify_offenses!(source, expected, actual)
      mapped_actual = actual.map do |o|
        {
          start_offset: o["start_offset"],
          end_offset: o["end_offset"],
          message: o["message"]
        }
      end

      sort_offenses!(expected)
      sort_offenses!(mapped_actual)

      if expected != mapped_actual
        exp_str = expected.map { |o| "[#{o[:start_offset]}..#{o[:end_offset]}]: #{o[:message]}" }.join("\n")
        act_str = mapped_actual.map { |o| "[#{o[:start_offset]}..#{o[:end_offset]}]: #{o[:message]}" }.join("\n")
        raise "Offenses mismatch!\n\nExpected:\n#{exp_str}\n\nActual:\n#{act_str}"
      end
    end

    def apply_edits_bytes(source, blob)
      return source if blob.nil? || blob.empty?

      edits = []
      pos = 0
      while pos < blob.bytesize
        # Extract format: "<start> <end> <replen> "
        rest = blob[pos..-1]

        sp1 = rest.index(" ")
        break unless sp1
        start_offset = rest[0...sp1].to_i

        rest2 = rest[sp1+1..-1]
        sp2 = rest2.index(" ")
        break unless sp2
        end_offset = rest2[0...sp2].to_i

        rest3 = rest2[sp2+1..-1]
        sp3 = rest3.index(" ")
        break unless sp3
        replen = rest3[0...sp3].to_i

        header_len = sp1 + 1 + sp2 + 1 + sp3 + 1
        pos += header_len

        replacement = blob[pos, replen]
        pos += replen

        edits << { start: start_offset, end: end_offset, replacement: replacement }
      end

      # Apply back-to-front to preserve offsets
      bytes = source.bytes
      edits.sort_by! { |e| -e[:start] }
      edits.each do |e|
        bytes[e[:start]...e[:end]] = e[:replacement].bytes
      end

      begin
        bytes.pack("C*")
      rescue
        bytes.map(&:chr).join
      end
    end
  end
end

def describe_cop(cop_name, &block)
  runner = Murphy::TestRunner.new(cop_name)
  runner.instance_eval(&block)
  if runner.failures.any?
    msg = "Test failed for #{cop_name}:\n" + runner.failures.join("\n")
    puts msg
    raise msg
  end
end
