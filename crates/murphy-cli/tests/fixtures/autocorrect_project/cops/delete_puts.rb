# DeletePpCop: removes the "pp" selector token (replaces with empty string)
# via fix.remove. Only matches bare pp calls (no receiver). Drives the
# delete-case fixture (delete_me.rb). Murphy/NoReceiverPuts does NOT fire
# on pp, so this cop produces the only offense on delete_me.rb.
class DeletePpCop < Murphy::Cop
  MSG = "Remove bare pp call selector"

  def on_call_node(node)
    return unless node.name == :pp && node.receiver_nil?
    msg_loc = node.message_loc
    return unless msg_loc
    add_offense(msg_loc, message: MSG) do |fix|
      fix.remove(msg_loc)
    end
  end
end
