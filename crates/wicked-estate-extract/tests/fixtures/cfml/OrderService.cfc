<cfcomponent name="OrderService" extends="BaseService">
    <cffunction name="placeOrder" access="public" returntype="any">
        <cfargument name="cart" type="struct" required="true">
        <cfset var total = calculateTotal(arguments.cart)>
        <cfset chargePayment(total)>
        <cfreturn total>
    </cffunction>

    <cffunction name="calculateTotal" access="private" returntype="numeric">
        <cfargument name="cart" type="struct">
        <cfreturn sumLineItems(arguments.cart.items)>
    </cffunction>

    <cffunction name="cancelOrder" access="public">
        <cfargument name="orderId" type="numeric">
        <cfset refundPayment(arguments.orderId)>
    </cffunction>
</cfcomponent>
