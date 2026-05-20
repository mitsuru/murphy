# PutsToLoggerCop: replaces the "puts" selector token with "logger.info".
# Only matches bare calls (no receiver). Drives the replace-case fixture.
class PutsToLoggerCop < Murphy::Cop
  MSG = "Use logger.info instead of bare puts"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    msg_loc = node.message_loc
    return unless msg_loc
    add_offense(msg_loc, message: MSG) do |fix|
      fix.replace(msg_loc, "logger.info")
    end
  end
end
