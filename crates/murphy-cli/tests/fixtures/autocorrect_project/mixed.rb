# Fixture: mixed — a puts (autocorrectable via PutsToLoggerCop) AND a print
# (Murphy/NoReceiverPuts fires, no autocorrect block). After --fix, the puts
# becomes logger.info but the print offense survives → post-fix exit 1.
puts "world"
print "residual"
